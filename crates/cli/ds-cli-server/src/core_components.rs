//! Core's local component commands over the protected native module owner.
//! No release authority, executable probe or process belongs to removal.
use ds_cli_contract::spec::{
    Arg, Authority, Chapter, Command, Effect, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Failure, Inputs};
use ds_command_kernel::{module_install::ModuleSelectionRecord, module_release::ModuleComponent};
use ds_update_runtime::modules::ModuleStore;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{path::Path, time::Duration};

const REMOVE_ROUTE: &str = "/v1/core/component/remove";
const MAX_REQUEST_BYTES: usize = 512;

pub(crate) const COMPONENT: Arg = Arg::value(
    "component",
    "<solar|reporter|network|interactions|command-kernel>",
    "Fixed component owner; UI components require runtime wasm.",
)
.choices(&[
    "solar",
    "reporter",
    "network",
    "interactions",
    "command-kernel",
])
.required();
pub(crate) const RUNTIME: Arg = Arg::value(
    "runtime",
    "<native|wasm>",
    "Native engine or UI WASM installation; native remains the default.",
)
.choices(&["native", "wasm"])
.default("native");

pub(crate) const CHANNEL: Arg = Arg::value(
    "channel",
    "<stable|canary>",
    "Exact installed component lane.",
)
.choices(&["stable", "canary"])
.required();

pub(crate) const REFUSALS: &[Refusal] = &[
    crate::PLATFORM,
    Refusal { code: "module_scope_invalid", when: "a local UI unit crosses release scope, platform or lane", remedy: "use explicit Linux canary local-dev UI scope; release selections admit only published units" },
    Refusal {
        code: "ui_component_platform_unavailable",
        when: "UI installation or reads run on an unqualified platform",
        remedy: "use Linux x86_64; Windows UI installation awaits its DACL adapter",
    },
    Refusal {
        code: "module_not_installed",
        when: "no UI component selection exists",
        remedy: "explicitly stage and activate a published UI unit",
    },
    Refusal {
        code: "module_artifact_invalid",
        when: "retained UI bytes or bounded member range differ from their immutable receipt",
        remedy: "retain the capture and repair or explicitly reinstall its exact UI unit",
    },
    Refusal {
        code: "module_release_component_invalid",
        when: "the runtime does not admit the selected component",
        remedy: "select native for Solar, Reporter or Network; select wasm for Interactions or UI Kernel",
    },
    Refusal {
        code: "invalid_choice",
        when: "component or channel is outside the declared closed choices",
        remedy: "select solar, reporter or network and stable or canary",
    },
    Refusal {
        code: "component_generation_invalid",
        when: "expected generation is not an unsigned decimal integer",
        remedy: "read core component selection and pass its generation, or zero when absent",
    },
    Refusal {
        code: "component_runtime_unavailable",
        when: "this lane's Core background runtime is absent, incompatible or not answering",
        remedy: "start this lane's Core under this OS account and retry; its absence never authorizes removal",
    },
    Refusal {
        code: "component_runtime_identity_mismatch",
        when: "the selected lane or native runtime identity differs from its protected connection",
        remedy: "retain the component and use the intended lane's owned Core background runtime",
    },
    Refusal {
        code: "component_removal_context_invalid",
        when: "Core's retained native owner context is incomplete or has an unsupported lane",
        remedy: "retain the component and restart Core with its intact protected native owner context",
    },
    Refusal {
        code: "component_request_invalid",
        when: "the typed removal request is malformed, excessive or has undeclared fields",
        remedy: "use the declared removal command with component, channel and generation",
    },
    Refusal {
        code: "component_uncaptured_jobs",
        when: "the owned Core runtime has pending work without a valid immutable engine capture",
        remedy: "finish or cancel the pending legacy work first; its stored input is kept",
    },
    Refusal {
        code: "module_removed",
        when: "the component has an authoritative removed selection and no active unit",
        remedy: "stage and activate a compatible published component at the removed generation",
    },
    Refusal {
        code: "module_rollback_unavailable",
        when: "no retained verified previous component unit is available",
        remedy: "retain the current selection or explicitly stage and activate a compatible published unit",
    },
    Refusal {
        code: "module_activation_deferred",
        when: "native observations retain dirty edits, uncaptured sessions or a required restart",
        remedy: "finish or checkpoint the owning native session before removing the component",
    },
    Refusal {
        code: "module_selection_changed",
        when: "the captured component generation differs from the protected current selection",
        remedy: "read core component selection again before confirming removal of that generation",
    },
    Refusal {
        code: "module_update_busy",
        when: "another native update holds this component and lane lock",
        remedy: "wait for the owning component update to finish",
    },
    Refusal {
        code: "module_store_invalid",
        when: "the native owned store is unavailable, linked, reparsed or has an unsafe path",
        remedy: "retain the selected lane and repair its protected native component store",
    },
    Refusal {
        code: "module_receipt_invalid",
        when: "the retained selection is corrupt, crossed, excessive or its generation is exhausted",
        remedy: "retain the selected lane and repair its exact native component receipt",
    },
    Refusal {
        code: "module_platform_unsupported",
        when: "the native host is outside the supported targets",
        remedy: "use a supported Linux or Windows x86_64 native host",
    },
    Refusal {
        code: "module_update_cancelled",
        when: "native cancellation interrupts removal before selection commit",
        remedy: "read the selection before starting another confirmed removal",
    },
    Refusal {
        code: "module_activation_failed",
        when: "the native owner cannot atomically replace the removed selection",
        remedy: "retain the selected lane and resolve the named native storage failure",
    },
    Refusal {
        code: "module_release_schema_invalid",
        when: "retained installation metadata has an unsupported schema",
        remedy: "repair the native installation receipt using its owner's supported schema",
    },
    Refusal {
        code: "module_release_identity_mismatch",
        when: "retained metadata crosses component, lane or product identity",
        remedy: "repair the exact component and lane installation receipt",
    },
    Refusal {
        code: "module_release_protocol_unsupported",
        when: "retained metadata has an unsupported owner protocol",
        remedy: "repair the installation receipt with the fixed owner's supported protocol",
    },
    Refusal {
        code: "module_release_target_unsupported",
        when: "retained metadata names an unsupported platform target",
        remedy: "repair the qualified native target installation receipt",
    },
    Refusal {
        code: "module_release_asset_invalid",
        when: "retained executable identity or bounds are invalid",
        remedy: "repair the qualified native executable installation receipt",
    },
    Refusal {
        code: "module_release_provenance_invalid",
        when: "retained producer provenance is incomplete or crossed",
        remedy: "repair the owner's complete native installation provenance",
    },
    Refusal {
        code: "module_release_url_invalid",
        when: "retained publication metadata has an unadmitted artifact URL",
        remedy: "repair the exact immutable native installation receipt",
    },
    Refusal {
        code: "module_release_invalid",
        when: "retained native publication facts fail owner admission",
        remedy: "repair the exact retained native installation receipt",
    },
    Refusal {
        code: "module_engine_incompatible",
        when: "retained engine facts differ from producer provenance",
        remedy: "repair the exact qualified native installation receipt",
    },
];

pub static REMOVE: Command = Command {
    id: "core.component.remove",
    path: &["core", "component", "remove"],
    contract: 2,
    summary: "Remove one managed component selection from this machine.",
    purpose: "Runtime wasm removes an installed UI selection locally, keeping its bytes and previous receipt for rollback, without login or a running Core. Native (default) confirms removal of one component and lane at an explicitly held generation through that lane's running Core: it fences job admission, checks that every pending job holds an immutable engine capture, and commits a removed-v2 selection. Uncaptured legacy jobs refuse; captured jobs and installed versions remain. Generation zero may fence an absent selection. Details: docs/reference/core.md.",
    chapter: Chapter::Workstation,
    effect: Effect::MachineWrite,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[
        COMPONENT,
        CHANNEL,
        RUNTIME,
        crate::ui_components::SCOPE,
        Arg::value(
            "expected-generation",
            "<u64>",
            "Exact generation returned by core component selection; zero when absent.",
        )
        .required(),
    ],
    output: "UI runtime wasm: committed ds.ui-module-selection/v1 removed state, previous unit kept. Native: committed ds.module-selection/v2 removal receipt with component, channel, incremented generation and state removed.",
    examples: &[],
    refusals: REFUSALS,
    reference: Some("docs/reference/core.md"),
    search: &["uninstall", "addon", "generation"],
    requires: Requires::Server,
    availability: crate::native_host_availability,
};

pub static SELECTION: Command = Command {
    id: "core.component.selection",
    path: &["core", "component", "selection"],
    contract: 2,
    summary: "Read a component's retained selection and generation offline.",
    purpose: "Read the protected local selection (installed-v1 or removed-v2) to capture the exact generation for removal; an absent record is null with generation zero. Reads only: no directories, permission changes, update lock, release query or executable probe, and no account or project authority.",
    chapter: Chapter::Workstation,
    effect: Effect::ReadOnly,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[COMPONENT, CHANNEL, RUNTIME, crate::ui_components::SCOPE],
    output: "UI runtime wasm: ds.ui-module-selection/v1 (active/previous or removed). Native: the retained ds.module-selection/v1 or v2 record with component, channel and generation, or null. It does not imply the executable is available.",
    examples: &[],
    refusals: REFUSALS,
    reference: Some("docs/reference/core.md"),
    search: &["installed", "removed"],
    requires: Requires::Server,
    availability: crate::module_updates::native_module_availability,
};

pub(crate) fn selected(inputs: &Inputs) -> Result<(ModuleComponent, &str), Failure> {
    let component = match inputs.require("component")? {
        "solar" => ModuleComponent::Solar,
        "reporter" => ModuleComponent::Reporter,
        "network" => ModuleComponent::Network,
        _ => {
            return Err(
                Failure::invalid("invalid_choice", "select solar, reporter or network")
                    .remedy("select solar, reporter or network and stable or canary"),
            );
        }
    };
    let channel = inputs.require("channel")?;
    if !matches!(channel, "stable" | "canary") {
        return Err(
            Failure::invalid("invalid_choice", "select stable or canary")
                .remedy("select solar, reporter or network and stable or canary"),
        );
    }
    Ok((component, channel))
}

pub(crate) fn owner_failure(message: String) -> Failure {
    let code = message.split(':').next().unwrap_or_default();
    let refusal = REFUSALS.iter().find(|r| r.code == code).unwrap_or_else(|| {
        REFUSALS
            .iter()
            .find(|r| r.code == "module_store_invalid")
            .unwrap()
    });
    let bounded: String = message.chars().take(1024).collect();
    let failure = match refusal.code {
        "module_selection_changed" | "module_update_busy" => {
            Failure::conflict(refusal.code, bounded)
        }
        "server_platform_unsupported"
        | "component_runtime_unavailable"
        | "component_uncaptured_jobs"
        | "module_activation_deferred"
        | "module_rollback_unavailable"
        | "module_removed"
        | "module_not_installed"
        | "ui_component_platform_unavailable"
        | "module_platform_unsupported" => Failure::unavailable(refusal.code, bounded),
        "module_scope_invalid"
        | "component_generation_invalid"
        | "component_request_invalid"
        | "invalid_choice"
        | "module_release_component_invalid"
        | "module_release_schema_invalid"
        | "module_release_identity_mismatch"
        | "module_release_protocol_unsupported"
        | "module_release_target_unsupported"
        | "module_release_asset_invalid"
        | "module_release_provenance_invalid"
        | "module_release_url_invalid"
        | "module_release_invalid" => Failure::invalid(refusal.code, bounded),
        "component_runtime_identity_mismatch" | "component_removal_context_invalid" => {
            Failure::unauthorized(refusal.code, bounded)
        }
        "module_store_invalid"
        | "module_receipt_invalid"
        | "module_update_cancelled"
        | "module_activation_failed"
        | "module_engine_incompatible"
        | "module_artifact_invalid" => Failure::failed(refusal.code, bounded),
        _ => Failure::failed("module_store_invalid", bounded),
    };
    failure.remedy(refusal.remedy)
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RemovalRequest {
    component: ModuleComponent,
    channel: String,
    expected_generation: u64,
}

fn removal_request(inputs: &Inputs) -> Result<RemovalRequest, Failure> {
    let (component, channel) = selected(inputs)?;
    let raw = inputs.require("expected-generation")?;
    let generation = raw.parse::<u64>().ok().filter(|_| !raw.is_empty() && raw.bytes().all(|b| b.is_ascii_digit()))
        .ok_or_else(|| owner_failure("component_generation_invalid: expected generation must be an unsigned decimal integer".into()))?;
    Ok(RemovalRequest {
        component,
        channel: channel.to_owned(),
        expected_generation: generation,
    })
}

pub fn remove(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
    if crate::ui_components::requested(inputs)? {
        return crate::ui_components::remove(inputs);
    }
    crate::require_native_host()?;
    let request = removal_request(inputs)?;
    request_runtime(&request)
}

fn runtime_unavailable(message: impl std::fmt::Display) -> Failure {
    owner_failure(format!("component_runtime_unavailable: {message}"))
}

fn request_runtime(request: &RemovalRequest) -> Result<Value, Failure> {
    let state = ds_compute_runtime::server_state_directory(&request.channel, None)
        .map_err(runtime_unavailable)?;
    let connection = crate::host::load_connection(&state).map_err(runtime_unavailable)?;
    if connection.lane != request.channel {
        return Err(owner_failure("component_runtime_identity_mismatch: protected runtime lane differs from the selected component lane".into()));
    }
    if connection.legacy_address.is_some() {
        return Err(runtime_unavailable(
            "the retained native connection predates the owner-only Core command transport",
        ));
    }
    let body = serde_json::to_vec(request)
        .map_err(|error| Failure::internal("module_result_invalid", error.to_string()))?;
    let reply = crate::transport::call(
        &connection.socket,
        "POST",
        REMOVE_ROUTE,
        &[("content-type", "application/json")],
        Some(&body),
        64 * 1024,
        Duration::from_secs(10),
    )
    .map_err(runtime_unavailable)?;
    let receipt = decode_reply(reply)?;
    if receipt["component"] != request.component.as_str()
        || receipt["channel"] != request.channel
        || receipt["generation"].as_u64() != request.expected_generation.checked_add(1)
    {
        return Err(owner_failure("module_receipt_invalid: Core removal receipt crosses the requested component, lane or generation".into()));
    }
    Ok(receipt)
}

fn decode_reply(reply: crate::transport::Reply) -> Result<Value, Failure> {
    let value: Value = serde_json::from_slice(&reply.body).map_err(runtime_unavailable)?;
    if reply.status >= 400 {
        let code = value["code"].as_str().unwrap_or_default();
        if REFUSALS.iter().any(|refusal| refusal.code == code) {
            return Err(owner_failure(format!(
                "{code}: {}",
                value["error"]
                    .as_str()
                    .unwrap_or("Core refused component removal")
            )));
        }
        return Err(runtime_unavailable(
            value["error"]
                .as_str()
                .unwrap_or("Core does not support this component removal request"),
        ));
    }
    let removed: ds_command_kernel::module_install::RemovedModuleSelection =
        serde_json::from_value(value).map_err(runtime_unavailable)?;
    if removed.schema != ds_command_kernel::module_install::REMOVED_SELECTION_SCHEMA
        || removed.generation == 0
    {
        return Err(owner_failure(
            "module_receipt_invalid: Core removal receipt has an unsupported schema or generation"
                .into(),
        ));
    }
    serde_json::to_value(removed)
        .map_err(|error| Failure::internal("module_result_invalid", error.to_string()))
}

fn decode_request(body: &[u8]) -> Result<RemovalRequest, Failure> {
    if body.len() > MAX_REQUEST_BYTES {
        return Err(owner_failure(
            "component_request_invalid: removal request exceeds 512 bytes".into(),
        ));
    }
    let request: RemovalRequest = serde_json::from_slice(body).map_err(|_| {
        owner_failure("component_request_invalid: use the closed component removal request".into())
    })?;
    if !matches!(request.channel.as_str(), "stable" | "canary") {
        return Err(owner_failure(
            "component_request_invalid: use an exact stable or canary channel".into(),
        ));
    }
    Ok(request)
}

// This route is private to the existing Core owner-only socket. Middleware
// admits the OS peer and the unchanged native owner before this handler runs.
pub(crate) async fn invoke(
    axum::extract::State(app): axum::extract::State<crate::host::App>,
    request: axum::extract::Request,
) -> Result<axum::Json<Value>, (axum::http::StatusCode, axum::Json<Value>)> {
    let body = axum::body::to_bytes(request.into_body(), MAX_REQUEST_BYTES)
        .await
        .map_err(|_| {
            crate::host::typed(&owner_failure(
                "component_request_invalid: removal request exceeds 512 bytes".into(),
            ))
        })?;
    let request = decode_request(&body).map_err(|failure| crate::host::typed(&failure))?;
    crate::host::admitting(move || {
        let identity = app.sessions.identity();
        if identity.owner != app.connection.owner || identity.lane() != app.connection.lane {
            return Err(owner_failure("component_runtime_identity_mismatch: native runtime identity differs from its protected connection".into()));
        }
        remove_on_runtime(&app.database, identity, request, ModuleStore::managed).map(axum::Json)
    }).await
}

fn remove_on_runtime(
    database: &Path,
    identity: &ds_compute_runtime::HostIdentity,
    request: RemovalRequest,
    open: impl FnOnce(ModuleComponent, &str) -> Result<ModuleStore, String>,
) -> Result<Value, Failure> {
    if request.channel != identity.lane() {
        return Err(owner_failure("component_runtime_identity_mismatch: component lane differs from the admitted native runtime lane".into()));
    }
    let removed = ds_compute_runtime::with_component_removal_admission(
        database,
        identity,
        request.component,
        |guard| {
            open(request.component, &request.channel)?.remove(
                request.expected_generation,
                guard,
                &|| false,
            )
        },
    )
    .map_err(owner_failure)?;
    serde_json::to_value(removed)
        .map_err(|error| Failure::internal("module_result_invalid", error.to_string()))
}

pub fn selection(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
    if crate::ui_components::requested(inputs)? {
        return crate::ui_components::selection(inputs);
    }
    ds_update_runtime::modules::current_platform().map_err(owner_failure)?;
    selection_with(inputs, ModuleStore::selection_record_managed)
}

fn selection_with(
    inputs: &Inputs,
    read: impl FnOnce(ModuleComponent, &str) -> Result<Option<ModuleSelectionRecord>, String>,
) -> Result<Value, Failure> {
    let (component, channel) = selected(inputs)?;
    let record = read(component, channel).map_err(owner_failure)?;
    serde_json::to_value(record)
        .map_err(|error| Failure::internal("module_result_invalid", error.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    use std::sync::Arc;
    use tower::ServiceExt;

    #[test]
    fn every_owner_refusal_is_declared_and_actionable() {
        for refusal in REFUSALS {
            let failure = owner_failure(format!("{}: owner refused", refusal.code));
            assert_eq!(failure.code(), refusal.code);
            assert_eq!(failure.remedy_text(), Some(refusal.remedy));
            assert!(REMOVE.refusals.iter().any(|r| r.code == failure.code()));
            assert!(!refusal.remedy.is_empty());
        }
        assert_eq!(
            owner_failure("unknown_owner_code: refusal".into()).code(),
            "module_store_invalid"
        );
    }

    fn inputs(generation: &str) -> Inputs {
        ds_cli_contract::parse(
            &REMOVE,
            &[
                "--component",
                "network",
                "--channel",
                "canary",
                "--expected-generation",
                generation,
            ]
            .map(str::to_owned),
        )
        .unwrap()
    }

    fn identity() -> ds_compute_runtime::HostIdentity {
        ds_compute_runtime::HostIdentity {
            owner: "fixture-owner".into(),
            principal: ds_command_kernel::execution_context::Principal {
                uid: "fixture-user".into(),
                lane: "canary".into(),
                deployment: "https://fixture.example".into(),
                install_id: "fixture-install".into(),
            },
        }
    }

    struct LocalOwner;
    impl ds_compute_runtime::Authorizer for LocalOwner {
        fn authorize(&self, _: &str) -> Result<(), String> {
            Ok(())
        }
    }

    fn app(path: &Path) -> crate::host::App {
        let identity = identity();
        let connection = crate::host::Connection {
            owner: identity.owner.clone(),
            lane: identity.lane().into(),
            socket: crate::transport::socket_path(path),
            legacy_address: None,
        };
        let database = path.join("store.sqlite");
        let sessions = crate::server_sync::sessions::ServerSessions::with(
            connection.clone(),
            database.clone(),
            crate::host::tests::limits(),
            identity,
            Arc::new(crate::host::tests::NoGateway),
        );
        crate::host::App {
            database,
            connection,
            os_account: Some(crate::transport::Account::Unix(crate::host::tests::OS_UID)),
            auth: Arc::new(LocalOwner),
            requests: Arc::new(crate::host::Door::new(4)),
            activity: None,
            sessions,
            solar: Arc::new(crate::solar_application::Applications::default()),
            profiles: Arc::new(crate::profile_sessions::Sessions::default()),
            layers: crate::layers::NativeLayerHost::fixture_native("canary"),
        }
    }

    async fn route(app: crate::host::App, body: Vec<u8>, owned: bool) -> (u16, Value) {
        let mut request = axum::http::Request::builder()
            .method("POST")
            .uri(REMOVE_ROUTE)
            .body(axum::body::Body::from(body))
            .unwrap();
        if owned {
            request
                .extensions_mut()
                .insert(crate::host::tests::OWNER_PEER);
        }
        let response = crate::host::router(app).oneshot(request).await.unwrap();
        let status = response.status().as_u16();
        let bytes = axum::body::to_bytes(response.into_body(), 64 * 1024)
            .await
            .unwrap();
        (status, serde_json::from_slice(&bytes).unwrap())
    }

    #[tokio::test]
    async fn actual_private_route_refuses_foreign_peers_and_uncaptured_legacy_jobs() {
        let base = tempfile::tempdir().unwrap();
        let app = app(base.path());
        let store = ds_compute_runtime::open(&app.database).unwrap();
        let legacy = serde_json::json!({
            "id":"a".repeat(64), "owner":identity().owner, "lane":"canary",
            "input_sha256":"b".repeat(64), "engine":"lv_processing",
            "input_tag":"ds.lv.process/v1", "phase":"queued", "attempts":0,
            "created_at_ms":1, "updated_at_ms":1, "worker":null,
            "lease_until_ms":0, "result_sha256":null, "error":null,
        });
        let row = serde_json::to_string(&legacy).unwrap();
        store.connection_for_test().execute(
            "INSERT INTO compute_jobs(id,owner,lane,phase,lease_until,created,row,input) VALUES(?1,?2,'canary','queued',0,1,?3,?4)",
            rusqlite::params!["a".repeat(64), identity().owner, row, b"retained input".as_slice()],
        ).unwrap();
        let body = serde_json::to_vec(&removal_request(&inputs("0")).unwrap()).unwrap();
        let (status, refusal) = route(app.clone(), body.clone(), false).await;
        assert_eq!(status, 401);
        assert_eq!(refusal["code"], "server_peer_refused");
        let (status, refusal) = route(app.clone(), body, true).await;
        assert_eq!(status, 503);
        assert_eq!(refusal["code"], "component_uncaptured_jobs");
        assert_eq!(
            store
                .connection_for_test()
                .query_row("SELECT row FROM compute_jobs", [], |row| row
                    .get::<_, String>(0))
                .unwrap(),
            row
        );
        for body in [b"{}".to_vec(), vec![b' '; MAX_REQUEST_BYTES + 1]] {
            let (status, refusal) = route(app.clone(), body, true).await;
            assert_eq!(status, 400);
            assert_eq!(refusal["code"], "component_request_invalid");
        }
    }

    #[test]
    fn command_declares_closed_machine_effect_and_central_confirmation() {
        assert_eq!(crate::DOMAIN.id, "core");
        assert_eq!(REMOVE.id, "core.component.remove");
        assert_eq!(REMOVE.path, ["core", "component", "remove"]);
        assert_eq!(REMOVE.effect, Effect::MachineWrite);
        assert_eq!(REMOVE.authority, Authority::None);
        assert!(REMOVE.confirmation_required_for(&inputs("0")));
        assert!(!SELECTION.requires_confirmation());
        assert_eq!(
            REMOVE.args.iter().map(|arg| arg.name).collect::<Vec<_>>(),
            ["component", "channel", "runtime", "expected-generation"]
        );
        assert_eq!(
            SELECTION
                .args
                .iter()
                .map(|arg| arg.name)
                .collect::<Vec<_>>(),
            ["component", "channel", "runtime"]
        );
        let descriptor = ds_cli_contract::help::command_json(&REMOVE);
        assert_eq!(descriptor["confirmation_required"], true);
        assert_eq!(descriptor["authority"], "none");
        for arguments in [
            vec![
                "--component",
                "desktop",
                "--channel",
                "canary",
                "--expected-generation",
                "0",
            ],
            vec![
                "--component",
                "network",
                "--channel",
                "other",
                "--expected-generation",
                "0",
            ],
            vec!["--component", "network", "--channel", "canary"],
            vec![
                "--component",
                "network",
                "--channel",
                "canary",
                "--expected-generation",
                "0",
                "--idle",
            ],
        ] {
            assert!(
                ds_cli_contract::parse(
                    &REMOVE,
                    &arguments.into_iter().map(str::to_owned).collect::<Vec<_>>()
                )
                .is_err()
            );
        }
    }

    #[test]
    fn malformed_generation_refuses_before_building_native_request() {
        for generation in ["", "-1", "+1", "1.0", " 1", "18446744073709551616"] {
            let error = removal_request(&inputs(generation)).err().unwrap();
            assert_eq!(error.code(), "component_generation_invalid");
        }
    }

    #[test]
    fn older_or_refused_runtime_never_becomes_idle_admission() {
        let refusal = decode_reply(crate::transport::Reply {
            status: 404,
            body: br#"{"error":"unsupported private route"}"#.to_vec(),
        })
        .unwrap_err();
        assert_eq!(refusal.code(), "component_runtime_unavailable");
        let refused = decode_reply(crate::transport::Reply {
            status: 409,
            body: br#"{"code":"component_uncaptured_jobs","error":"legacy job is pending"}"#
                .to_vec(),
        })
        .unwrap_err();
        assert_eq!(refused.code(), "component_uncaptured_jobs");
        assert!(refused.remedy_text().is_some());
    }

    #[test]
    fn native_request_is_closed_bounded_and_lane_fenced_before_effects() {
        for body in [
            serde_json::json!({"component":"network","channel":"canary","expectedGeneration":0,"idle":true}),
            serde_json::json!({"component":"desktop","channel":"canary","expectedGeneration":0}),
            serde_json::json!({"component":"network","channel":"other","expectedGeneration":0}),
            serde_json::json!({"component":"network","channel":"canary","expectedGeneration":-1}),
        ] {
            assert_eq!(
                decode_request(&serde_json::to_vec(&body).unwrap())
                    .err()
                    .unwrap()
                    .code(),
                "component_request_invalid"
            );
        }
        assert_eq!(
            decode_request(&vec![b' '; MAX_REQUEST_BYTES + 1])
                .err()
                .unwrap()
                .code(),
            "component_request_invalid"
        );
        let base = tempfile::tempdir().unwrap();
        let database = base.path().join("not-created.sqlite");
        let opened = Cell::new(false);
        let mut request = removal_request(&inputs("0")).unwrap();
        request.channel = "stable".into();
        assert_eq!(
            remove_on_runtime(&database, &identity(), request, |_, _| {
                opened.set(true);
                panic!("crossed lane must not open module store")
            })
            .unwrap_err()
            .code(),
            "component_runtime_identity_mismatch"
        );
        assert!(!opened.get());
        assert!(!database.exists());
    }

    #[test]
    fn removal_returns_owner_receipt_retains_versions_and_fences_stale_generation() {
        let base = tempfile::tempdir().unwrap();
        let runtime = tempfile::tempdir().unwrap();
        let database = runtime.path().join("store.sqlite");
        let open = |component, channel: &str| ModuleStore::new(base.path(), component, channel);
        let store = open(ModuleComponent::Network, "canary").unwrap();
        assert!(store.selection_record().unwrap().is_none());
        let captured_version = base.path().join("network/canary/versions/captured-job");
        std::fs::create_dir(&captured_version).unwrap();
        let executable = captured_version.join("ds-network");
        std::fs::write(&executable, b"already captured executable").unwrap();
        let receipt = remove_on_runtime(
            &database,
            &identity(),
            removal_request(&inputs("0")).unwrap(),
            open,
        )
        .unwrap();
        assert_eq!(
            receipt,
            serde_json::json!({
                "schema": "ds.module-selection/v2", "component": "network",
                "channel": "canary", "generation": 1, "state": "removed",
            })
        );
        assert_eq!(
            serde_json::to_value(store.selection_record().unwrap()).unwrap(),
            receipt
        );
        let read_inputs = ds_cli_contract::parse(
            &SELECTION,
            &["--component", "network", "--channel", "canary"].map(str::to_owned),
        )
        .unwrap();
        assert_eq!(
            selection_with(&read_inputs, |component, channel| {
                assert_eq!(component, ModuleComponent::Network);
                assert_eq!(channel, "canary");
                store.selection_record()
            })
            .unwrap(),
            receipt
        );
        assert_eq!(
            std::fs::read(&executable).unwrap(),
            b"already captured executable"
        );
        assert!(
            store
                .selected_executable(&|| false)
                .unwrap_err()
                .starts_with("module_removed:")
        );
        let record = base.path().join("network/canary/selection.json");
        let before = std::fs::read(&record).unwrap();
        assert_eq!(
            remove_on_runtime(
                &database,
                &identity(),
                removal_request(&inputs("0")).unwrap(),
                open
            )
            .unwrap_err()
            .code(),
            "module_selection_changed"
        );
        assert_eq!(std::fs::read(&record).unwrap(), before);
        assert_eq!(
            remove_on_runtime(
                &database,
                &identity(),
                removal_request(&inputs("1")).unwrap(),
                open
            )
            .unwrap()["generation"],
            2
        );
        assert_eq!(
            std::fs::read(&executable).unwrap(),
            b"already captured executable"
        );
    }

    #[test]
    fn corrupt_selection_is_preserved_and_owner_refusal_is_actionable() {
        let base = tempfile::tempdir().unwrap();
        let runtime = tempfile::tempdir().unwrap();
        let database = runtime.path().join("store.sqlite");
        let open = |component, channel: &str| ModuleStore::new(base.path(), component, channel);
        open(ModuleComponent::Network, "canary").unwrap();
        let path = base.path().join("network/canary/selection.json");
        std::fs::write(&path, b"{corrupt native record").unwrap();
        let error = remove_on_runtime(
            &database,
            &identity(),
            removal_request(&inputs("0")).unwrap(),
            open,
        )
        .unwrap_err();
        assert_eq!(error.code(), "module_receipt_invalid");
        assert!(error.remedy_text().is_some());
        assert_eq!(std::fs::read(&path).unwrap(), b"{corrupt native record");
        let bounded = owner_failure("unexpected native failure".repeat(1000));
        assert_eq!(bounded.code(), "module_store_invalid");
        assert_eq!(bounded.message().chars().count(), 1024);
    }
}
