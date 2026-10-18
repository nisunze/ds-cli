//! Process-lifetime Profile ownership. Parsing/engineering stay in their existing
//! Grid exchange and managed Network owners; no browser, SDK or caller argv.
use base64::{Engine, engine::general_purpose::STANDARD};
use ds_cli_contract::Failure;
use ds_network_client::{CapturedNetworkClient, NetworkFailure, ProfileSession};
use ds_network_contracts::engine::NetworkCapability;
use ds_network_contracts::profile::NativeProfileGridInkV1;
use ds_network_contracts::profile::{
    NativeProfileEngineeringTableV1, NativeProfileOperationV1, NativeProfilePickRequestV1,
    NativeProfileSelectionRangeRequestV1, NativeProfileViewInputV1, PROFILE_VIEW_INPUT_SCHEMA,
};
use ds_network_contracts::profile_control::ProfileControlOperation;
use ds_sync_store::{ProfileSourceHeader, ProfileSourceScope, Store};
mod controls;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    path::Path,
    sync::Mutex,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

pub const REQUEST_SCHEMA: &str = "ds.core.profile-session/v1";
pub const RECEIPT_SCHEMA: &str = "ds.core.profile-receipt/v1";
pub const MAX_REQUEST: usize = 48 * 1024 * 1024;
pub const MAX_PACKAGE: usize = ds_network_contracts::profile::MAX_PROFILE_PACKAGE_BYTES;
const MAX_SESSIONS: usize = 8;
const IDLE: Duration = Duration::from_secs(15 * 60);
const DEADLINE: Duration = Duration::from_secs(30);

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Scope {
    pub principal: String,
    pub lane: String,
    pub deployment: String,
    pub install: String,
    pub project: String,
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub schema: String,
    pub operation: Operation,
}
#[derive(Debug, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum Operation {
    Open {
        package_base64: String,
        package_sha256: String,
        #[serde(default)]
        recover: bool,
    },
    PackageCheckpoint {
        session_id: String,
        model_id: String,
        expected_history: Value,
    },
    EditMode {
        session_id: String,
        model_id: String,
        expected_history: Value,
        enabled: bool,
    },
    Control {
        session_id: String,
        model_id: String,
        expected_history: Value,
        operation: ProfileControlOperation,
    },
    SaveControl {
        session_id: String,
        model_id: String,
        expected_history: Value,
        operation: Value,
    },
    Discard {
        session_id: String,
        model_id: String,
        expected_history: Value,
        request: Value,
    },
    Snapshot {
        session_id: String,
        model_id: String,
        expected_history: Value,
    },
    State {
        session_id: String,
        model_id: String,
        expected_history: Value,
    },
    Checkpoint {
        session_id: String,
        model_id: String,
        expected_history: Value,
    },
    View {
        session_id: String,
        model_id: String,
        expected_history: Value,
        options: Value,
        patch: Value,
        style_document: Value,
        cached: bool,
        #[serde(default)]
        observe: bool,
    },
    FeatureCodes {
        session_id: String,
        model_id: String,
        expected_history: Value,
    },
    Pick {
        session_id: String,
        model_id: String,
        expected_history: Value,
        request: NativeProfilePickRequestV1,
    },
    SelectionRange {
        session_id: String,
        model_id: String,
        expected_history: Value,
        request: NativeProfileSelectionRangeRequestV1,
    },
    Properties {
        session_id: String,
        model_id: String,
        expected_history: Value,
        entity_id: String,
    },
    EngineeringAttributes {
        session_id: String,
        model_id: String,
        expected_history: Value,
        table: NativeProfileEngineeringTableV1,
    },
    GisProjection {
        session_id: String,
        model_id: String,
        expected_history: Value,
        label_fields: Vec<String>,
    },
    Scene {
        session_id: String,
        model_id: String,
        expected_history: Value,
        options: Value,
        label_policy: Value,
        grid_ink: NativeProfileGridInkV1,
        cached: bool,
    },
    Close {
        session_id: String,
        model_id: String,
        expected_history: Value,
    },
}
impl Request {
    pub fn parse(bytes: &[u8]) -> Result<Self, Failure> {
        if bytes.len() > MAX_REQUEST {
            return Err(invalid("Profile request exceeds 48 MiB"));
        }
        let request: Self = serde_json::from_slice(bytes)
            .map_err(|_| invalid("Use the closed Profile session request"))?;
        if request.schema != REQUEST_SCHEMA {
            return Err(invalid("Profile request schema is unsupported"));
        }
        let limit = match &request.operation {
            Operation::Open { .. } => MAX_REQUEST,
            Operation::Control { .. } => {
                ds_network_contracts::profile::MAX_PROFILE_CONTROL_INPUT_BYTES
            }
            _ => ds_network_contracts::profile::MAX_PROFILE_CONTROL_BYTES,
        };
        if bytes.len() > limit {
            return Err(invalid(format!(
                "Profile operation exceeds its {limit}-byte bound"
            )));
        }
        Ok(request)
    }
}
fn invalid(message: impl ToString) -> Failure {
    Failure::invalid("core_profile_request_invalid", message.to_string())
        .remedy("Use core.profile-session contract 1 with an exact package SHA or captured session/model/history")
}
fn package_native(error: NetworkFailure) -> Failure {
    if error.code() == "network_engine_refused"
        && error.owner_code() == Some("profile_package_invalid")
    {
        invalid(error.message)
    } else {
        native(error)
    }
}
fn view_native(error: NetworkFailure) -> Failure {
    if error.code() == "network_engine_refused"
        && error.owner_code() == Some("profile_view_invalid")
    {
        // Same prepare/project failure class as the former linked adapter.
        invalid(error.message)
    } else {
        native(error)
    }
}
fn native(error: NetworkFailure) -> Failure {
    let code = error
        .owner_code()
        .unwrap_or_else(|| error.code())
        .to_owned();
    Failure::failed("core_profile_native_refused", error.to_string())
        .remedy("Retain the exact package/history and follow the native_code refusal; install an admitted compatible same-lane Network component if required")
        .detail(json!({"native_code":code,"native_remedy":error.owner_remedy,"native_detail":error.owner_detail}))
}

fn sha(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn decode_package(encoded: &str, expected: &str) -> Result<Vec<u8>, Failure> {
    if encoded.len() > MAX_PACKAGE.div_ceil(3) * 4
        || expected.len() != 64
        || !expected
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(invalid("Package bounds or lowercase SHA-256 are invalid"));
    }
    let bytes = STANDARD
        .decode(encoded)
        .map_err(|_| invalid("Package encoding is invalid"))?;
    if bytes.is_empty() || bytes.len() > MAX_PACKAGE || sha(&bytes) != expected {
        return Err(invalid("Package bytes differ from their exact SHA-256"));
    }
    Ok(bytes)
}
fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis().min(u64::MAX as u128) as u64)
}
fn source_scope(scope: &Scope) -> ProfileSourceScope {
    ProfileSourceScope {
        principal: scope.principal.clone(),
        lane: scope.lane.clone(),
        deployment: scope.deployment.clone(),
        install: scope.install.clone(),
        project: scope.project.clone(),
    }
}
fn source_error(error: ds_sync_store::Error) -> Failure {
    Failure::failed("core_profile_source_unavailable",error.to_string()).remedy("Preserve the exact original and history; inspect the protected Core SQLite source cache without resetting authored models")
}
fn operation_owner(operation: &Operation) -> Option<(&str, &str, &Value)> {
    match operation {
        Operation::Open { .. } => None,
        Operation::Snapshot {
            session_id,
            model_id,
            expected_history,
        }
        | Operation::PackageCheckpoint {
            session_id,
            model_id,
            expected_history,
        }
        | Operation::EditMode {
            session_id,
            model_id,
            expected_history,
            ..
        }
        | Operation::Control {
            session_id,
            model_id,
            expected_history,
            ..
        }
        | Operation::SaveControl {
            session_id,
            model_id,
            expected_history,
            ..
        }
        | Operation::Discard {
            session_id,
            model_id,
            expected_history,
            ..
        }
        | Operation::State {
            session_id,
            model_id,
            expected_history,
        }
        | Operation::Checkpoint {
            session_id,
            model_id,
            expected_history,
        }
        | Operation::Close {
            session_id,
            model_id,
            expected_history,
        }
        | Operation::View {
            session_id,
            model_id,
            expected_history,
            ..
        }
        | Operation::Scene {
            session_id,
            model_id,
            expected_history,
            ..
        }
        | Operation::GisProjection {
            session_id,
            model_id,
            expected_history,
            ..
        }
        | Operation::FeatureCodes {
            session_id,
            model_id,
            expected_history,
        }
        | Operation::Pick {
            session_id,
            model_id,
            expected_history,
            ..
        }
        | Operation::SelectionRange {
            session_id,
            model_id,
            expected_history,
            ..
        }
        | Operation::Properties {
            session_id,
            model_id,
            expected_history,
            ..
        }
        | Operation::EngineeringAttributes {
            session_id,
            model_id,
            expected_history,
            ..
        } => Some((session_id, model_id, expected_history)),
    }
}
fn open_original(
    scope: Scope,
    scratch: &Path,
    bytes: &[u8],
    id: &str,
    authorized: &dyn Fn() -> Result<(), Failure>,
    required_capability: Option<NetworkCapability>,
    retained_pin: Option<&ds_network_client::NetworkEnginePin>,
) -> Result<(Held, Vec<u8>), Failure> {
    let workers = std::thread::available_parallelism()
        .map_or(1, usize::from)
        .min(2);
    let client = match retained_pin {
        Some(pin) => CapturedNetworkClient::from_pin(pin, &|| false),
        None => CapturedNetworkClient::capture_for_lane(&scope.lane, workers, &|| false),
    }
    .map_err(native)?;
    authorized()?;
    if let Some(required) = required_capability {
        require_read_capability(&client.identity().capabilities, required)?;
    }
    require_read_capability(
        &client.identity().capabilities,
        NetworkCapability::ProfilePackageOpenV1,
    )?;
    require_read_capability(
        &client.identity().capabilities,
        NetworkCapability::ProfileSessionV1,
    )?;
    let prepared = client
        .prepare_profile_open(bytes, scratch, DEADLINE, &|| false)
        .map_err(package_native)?;
    authorized()?;
    let prepared = prepared.result;
    let source_revision = prepared.source_revision;
    let coordinate_system = prepared.coordinate_system;
    let input = serde_json::to_vec(&prepared.open).map_err(invalid)?;
    let mut session = client
        .open_profile_session_with_timeout(&input, scratch, DEADLINE, &|| false)
        .map_err(native)?;
    let result = session
        .request_at(
            NativeProfileOperationV1::Snapshot,
            Some(session.history().clone()),
            DEADLINE,
            &|| false,
        )
        .map_err(native)?;
    authorized()?;
    let source = ProfileSourceHeader {
        scope: source_scope(&scope),
        schema: "ds.core.profile-source/v1".into(),
        session_id: id.into(),
        model_id: session.model_id().into(),
        package_sha256: sha(bytes),
        package_bytes: bytes.len() as u64,
        initial_history: session.history().clone(),
        source_revision,
        coordinate_system: coordinate_system.clone(),
        engine: serde_json::to_value(session.pin()).map_err(invalid)?,
    };
    let value = Held {
        scope,
        package_sha256: source.package_sha256.clone(),
        source_revision,
        coordinate_system,
        retained_asset_count: serde_json::from_slice::<Value>(&result.bytes)
            .map_err(|_| invalid("Native snapshot changed"))?["retained_asset_count"]
            .as_u64()
            .ok_or_else(|| invalid("Native snapshot has no asset count"))?
            as usize,
        display_state: Default::default(),
        style: None,
        touched: Instant::now(),
        retained_history: source.initial_history.clone(),
        has_draft: false,
        saving: false,
        source,
        session,
    };
    Ok((value, result.bytes))
}
fn closed_source_receipt(scope: &Scope, source: &ProfileSourceHeader) -> Value {
    json!({"schema":RECEIPT_SCHEMA,"session_id":source.session_id,"project":scope.project,"lane":scope.lane,"package_sha256":source.package_sha256,"source_revision":source.source_revision,"coordinate_system":source.coordinate_system,"model_id":source.model_id,"history":source.initial_history,"engine":source.engine,"closed":true,"rehydrated":false,"payload":null})
}
const MAX_STYLE_BYTES: usize = 64 * 1024;
#[derive(Clone)]
struct StyleCapture {
    identity: ds_cli_auth::ProviderIdentity,
    document: Value,
}
struct StyleCatalogue {
    identity: ds_cli_auth::ProviderIdentity,
    lane: String,
    project: String,
    document: Value,
}
fn style_error(message: impl ToString) -> Failure {
    Failure::failed("core_profile_styles_unavailable", message.to_string())
        .remedy("Read the same project's authored ds_grid/profile Style Center document under the same native Core account; do not substitute default pens")
}
fn style_identity(scope: &Scope, identity: &ds_cli_auth::ProviderIdentity) -> Result<(), Failure> {
    if identity.uid() != scope.principal || identity.lane() != scope.lane {
        return Err(style_error(
            "Profile style identity differs from its native account/lane",
        ));
    }
    Ok(())
}
/// Null alone demands the existing authenticated catalogue. Present documents,
/// including malformed ones, remain the caller's exact input to physical admission.
fn resolve_style(
    scope: &Scope,
    supplied: Value,
    cached: Option<&StyleCapture>,
    authorized: &dyn Fn() -> Result<(), Failure>,
    identity: &dyn Fn() -> Result<ds_cli_auth::ProviderIdentity, Failure>,
    fetch: &dyn Fn() -> Result<StyleCatalogue, Failure>,
) -> Result<(Value, Option<StyleCapture>), Failure> {
    if !supplied.is_null() {
        return Ok((supplied, None));
    }
    authorized()?;
    let before = identity()?;
    style_identity(scope, &before)?;
    let capture = if let Some(cached) = cached {
        if cached.identity != before {
            return Err(style_error(
                "The captured Profile style audience/account changed",
            ));
        }
        cached.clone()
    } else {
        let catalogue = fetch()?;
        if catalogue.identity != before {
            return Err(style_error(
                "The returned Profile style catalogue has another captured native owner",
            ));
        }
        if catalogue.lane != scope.lane
            || catalogue.project != scope.project
            || catalogue.document["project_id"] != scope.project
        {
            return Err(style_error(
                "Profile style catalogue belongs to another project/lane",
            ));
        }
        let document = catalogue.document["styles"]["ds_grid/profile"].clone();
        if !document.is_object() {
            return Err(style_error(
                "The authored ds_grid/profile document is missing",
            ));
        }
        if serde_json::to_vec(&document).map_err(invalid)?.len() > MAX_STYLE_BYTES {
            return Err(style_error("The authored Profile style exceeds 64 KiB"));
        }
        StyleCapture {
            identity: before.clone(),
            document,
        }
    };
    authorized()?;
    if identity()? != before {
        return Err(style_error(
            "Native account/lane/audience changed during Profile style demand",
        ));
    }
    Ok((capture.document.clone(), Some(capture)))
}
fn adopt_style_capture(
    target: &mut Option<StyleCapture>,
    capture: Option<StyleCapture>,
    authorized: &dyn Fn() -> Result<(), Failure>,
    identity: &dyn Fn() -> Result<ds_cli_auth::ProviderIdentity, Failure>,
) -> Result<(), Failure> {
    authorized()?;
    if let Some(capture) = capture.as_ref() {
        if identity()? != capture.identity {
            return Err(style_error(
                "Native account/lane/audience changed before Profile style adoption",
            ));
        }
    }
    // A successful supplied successor deliberately clears the older null-demand
    // capture; failed preparation never reaches this adoption boundary.
    *target = capture;
    Ok(())
}
struct Held {
    scope: Scope,
    package_sha256: String,
    source_revision: u64,
    coordinate_system: String,
    retained_asset_count: usize,
    display_state: ds_command_kernel::profile_display::DisplayState,
    style: Option<StyleCapture>,
    touched: Instant,
    source: ProfileSourceHeader,
    retained_history: Value,
    has_draft: bool,
    saving: bool,
    session: ProfileSession,
}
#[derive(Default)]
pub struct Sessions {
    held: Mutex<BTreeMap<String, Held>>,
}
impl Sessions {
    pub fn execute(
        &self,
        scope: Scope,
        scratch: &Path,
        database: &Path,
        request: Request,
        authorized: &dyn Fn() -> Result<(), Failure>,
    ) -> Result<Value, Failure> {
        authorized()?;
        if request.schema != REQUEST_SCHEMA {
            return Err(invalid("Profile request schema is unsupported"));
        }
        if !matches!(scope.lane.as_str(), "stable" | "canary")
            || scope.principal.is_empty()
            || scope.deployment.is_empty()
            || scope.install.is_empty()
            || scope.project.is_empty()
        {
            return Err(invalid(
                "Profile needs a captured native account/lane/deployment/install/project",
            ));
        }
        let required_capability = read_capability(&request.operation);
        if required_capability.is_some() {
            // Admit all typed arguments before retained lookup/rebind can stage
            // or launch a worker. CRS is supplied only by the held original.
            let _ = read_projection_operation(&request.operation, "")?;
        }
        let mut held = self.held.lock().map_err(|_| {
            Failure::internal(
                "core_profile_session_unavailable",
                "Profile session ownership is poisoned",
            )
        })?;
        // Dropping a native session aborts/reaps its entire supervised worker.
        held.retain(|_, value| {
            // Original-only expiry may never silently discard authored history.
            !value.session.is_closed()
                && (value.saving
                    || value.session.history() != &value.source.initial_history
                    || value.touched.elapsed() < IDLE)
        });
        let mut rehydrated = false;
        if let Some((id, model, expected)) = operation_owner(&request.operation) {
            if !held.contains_key(id) {
                let source_scope = source_scope(&scope);
                let source = Store::open_read_only(database)
                    .map_err(source_error)?
                    .map(|store| store.profile_recovery(&source_scope, id))
                    .transpose()
                    .map_err(source_error)?
                    .flatten();
                if let Some(recovery) = source {
                    let header = recovery.source.clone();
                    let retained_history = recovery
                        .draft
                        .as_ref()
                        .map_or(&header.initial_history, |(draft, _)| &draft.history);
                    if header.model_id != model || retained_history != expected {
                        return Err(Failure::conflict(
                            "core_profile_history_conflict",
                            "Retained original differs from the exact requested model/initial history",
                        ));
                    }
                    authorized()?;
                    if matches!(request.operation, Operation::Close { .. })
                        && recovery.draft.is_none()
                    {
                        Store::open(database)
                            .map_err(source_error)?
                            .remove_profile_source(&source_scope, &header)
                            .map_err(source_error)?;
                        authorized()?;
                        return Ok(closed_source_receipt(&scope, &header));
                    }
                    let pin: ds_network_client::NetworkEnginePin =
                        serde_json::from_value(header.engine.clone()).map_err(invalid)?;
                    require_read_capability(
                        &pin.identity.capabilities,
                        NetworkCapability::ProfilePackageOpenV1,
                    )?;
                    if let Some(required) = required_capability.clone() {
                        // Source identity is admitted before staging/recreating any
                        // worker; a newer current executable cannot substitute for it.
                        require_read_capability(&pin.identity.capabilities, required)?;
                    }
                    if held.len() >= MAX_SESSIONS {
                        return Err(Failure::unavailable(
                            "core_profile_capacity",
                            "Core already retains eight Profile sessions",
                        ));
                    }
                    let (value, _) = if recovery.draft.is_some() {
                        controls::restore(
                            scope.clone(),
                            scratch,
                            &recovery,
                            authorized,
                            required_capability.clone(),
                        )?
                    } else {
                        open_original(
                            scope.clone(),
                            scratch,
                            &recovery.source_bytes,
                            id,
                            authorized,
                            required_capability.clone(),
                            Some(&pin),
                        )?
                    };
                    if value.source != header {
                        return Err(Failure::conflict(
                            "core_profile_source_changed",
                            "Exact original model/history/CRS/managed engine changed during rebind",
                        ));
                    }
                    authorized()?;
                    if recovery.draft.is_none() {
                        Store::open(database)
                            .map_err(source_error)?
                            .touch_profile_source(&source_scope, &header, now_ms())
                            .map_err(source_error)?;
                    }
                    authorized()?;
                    held.insert(id.to_owned(), value);
                    rehydrated = true;
                }
            }
        }
        let mut answer = match request.operation {
            operation @ (Operation::PackageCheckpoint { .. }
            | Operation::EditMode { .. }
            | Operation::Control { .. }
            | Operation::SaveControl { .. }
            | Operation::Discard { .. }) => {
                controls::execute(&mut held, &scope, database, operation, authorized)
            }
            Operation::Open {
                package_base64,
                package_sha256,
                recover,
            } => {
                let bytes = decode_package(&package_base64, &package_sha256)?;
                if recover {
                    if let Some(recovery) = Store::open_read_only(database)
                        .map_err(source_error)?
                        .map(|store| {
                            store.profile_draft_for_package(&source_scope(&scope), &package_sha256)
                        })
                        .transpose()
                        .map_err(source_error)?
                        .flatten()
                    {
                        if recovery.source_bytes != bytes {
                            return Err(invalid(
                                "Recovery source bytes differ from the opened exact package",
                            ));
                        }
                        return controls::resume(&mut held, scope, scratch, &recovery, authorized);
                    }
                }
                if held.len() >= MAX_SESSIONS {
                    return Err(Failure::unavailable(
                        "core_profile_capacity",
                        "Core already retains eight Profile sessions",
                    )
                    .remedy("Close an existing Profile session before opening another"));
                }
                let mut random = [0u8; 16];
                getrandom::getrandom(&mut random).map_err(|_| {
                    Failure::internal(
                        "core_profile_session_unavailable",
                        "Cannot allocate a private Profile handle",
                    )
                })?;
                let id = random
                    .iter()
                    .map(|b| format!("{b:02x}"))
                    .collect::<String>();
                if held.contains_key(&id) {
                    return Err(Failure::internal(
                        "core_profile_session_unavailable",
                        "Private Profile handle collision",
                    ));
                }
                let (value, snapshot) =
                    open_original(scope.clone(), scratch, &bytes, &id, authorized, None, None)?;
                authorized()?;
                let active: Vec<_> = held.keys().cloned().collect();
                Store::open(database)
                    .map_err(source_error)?
                    .retain_profile_source(
                        &source_scope(&scope),
                        &value.source,
                        &bytes,
                        &active,
                        now_ms(),
                    )
                    .map_err(source_error)?;
                authorized()?;
                let receipt = receipt(&id, &value, &snapshot, false)?;
                held.insert(id, value);
                Ok(receipt)
            }
            operation @ (Operation::FeatureCodes { .. }
            | Operation::EngineeringAttributes { .. }
            | Operation::GisProjection { .. }
            | Operation::Properties { .. }
            | Operation::Pick { .. }
            | Operation::SelectionRange { .. }) => {
                let (id, model, expected) =
                    operation_owner(&operation).expect("Closed read projection owner");
                let session = owned(&mut held, &scope, id, model, expected)?;
                require_read_capability(
                    &session.session.pin().identity.capabilities,
                    read_capability(&operation).expect("Closed read capability"),
                )?;
                let native_op = read_projection_operation(&operation, &session.coordinate_system)?
                    .expect("Closed read projection");
                let is_gis = matches!(&native_op, NativeProfileOperationV1::GisProjection { .. });
                let property_entity = match &operation {
                    Operation::Properties { entity_id, .. } => Some(entity_id.as_str()),
                    _ => None,
                };
                let result = session
                    .session
                    .request_at(native_op, Some(expected.clone()), DEADLINE, &|| false)
                    .map_err(native)?;
                authorized()?;
                validate_selection_payload(
                    &operation,
                    &result.bytes,
                    &session.session.history()["model_revision"],
                )?;
                if is_gis {
                    let projected: Value =
                        serde_json::from_slice(&result.bytes).map_err(invalid)?;
                    if projected["revision"] != session.session.history()["model_revision"]
                        || projected["crs"] != session.coordinate_system
                    {
                        return Err(invalid(
                            "Native GIS projection differs from retained revision/declared CRS",
                        ));
                    }
                }
                if let Some(entity_id) = property_entity {
                    validate_properties_payload(
                        &result.bytes,
                        entity_id,
                        &session.session.history()["model_revision"],
                    )?;
                }
                session.touched = Instant::now();
                receipt(id, session, &result.bytes, false)
            }
            Operation::View {
                session_id,
                model_id,
                expected_history,
                options,
                patch,
                style_document,
                cached,
                observe,
            } => {
                let value = owned(&mut held, &scope, &session_id, &model_id, &expected_history)?;
                require_read_capability(
                    &value.session.pin().identity.capabilities,
                    NetworkCapability::ProfileViewV1,
                )?;
                if observe {
                    require_read_capability(
                        &value.session.pin().identity.capabilities,
                        NetworkCapability::ProfileObserveV1,
                    )?;
                    if !cached {
                        return Err(invalid(
                            "Observe requires cached intent; reconcile is an explicit request",
                        ));
                    }
                }
                let options: ds_network_contracts::profile_scene::ProfileAtlasOptions =
                    serde_json::from_value(options)
                        .map_err(|_| invalid("Use existing native ProfileAtlasOptions"))?;
                let patch: ds_command_kernel::profile_display::DisplayPatch =
                    serde_json::from_value(patch)
                        .map_err(|_| invalid("Use existing native DisplayPatch"))?;
                let expected_display = value.display_state.patched(&patch).map_err(invalid)?;
                let native_identity =
                    || ds_cli_auth::headless_identity_for_named_project(&scope.lane);
                let (style_document, style_capture) = resolve_style(
                    &scope,
                    style_document,
                    value.style.as_ref(),
                    authorized,
                    &native_identity,
                    &|| {
                        let catalogue = ds_cli_auth::style_catalog(&scope.lane, &scope.project)?;
                        Ok(StyleCatalogue {
                            identity: catalogue.identity().clone(),
                            lane: catalogue.lane().into(),
                            project: catalogue.project_id().into(),
                            document: catalogue.result().document().clone(),
                        })
                    },
                )?;
                let input = NativeProfileViewInputV1 {
                    schema: PROFILE_VIEW_INPUT_SCHEMA.into(),
                    display_state: serde_json::to_value(&value.display_state).map_err(invalid)?,
                    options: serde_json::to_value(options).map_err(invalid)?,
                    patch: serde_json::to_value(patch).map_err(invalid)?,
                    style_document,
                };
                let result = if observe {
                    value.session.observe_view_at(
                        &input,
                        expected_history.clone(),
                        DEADLINE,
                        &|| false,
                    )
                } else {
                    value.session.view_at(
                        &input,
                        cached,
                        expected_history.clone(),
                        DEADLINE,
                        &|| false,
                    )
                }
                .map_err(view_native)?;
                authorized()?;
                let display = validate_view_payload(
                    &result.bytes,
                    &model_id,
                    &expected_history,
                    value.retained_asset_count,
                    &expected_display,
                )?;
                adopt_style_capture(
                    &mut value.style,
                    style_capture,
                    authorized,
                    &native_identity,
                )?;
                value.display_state = display;
                value.touched = Instant::now();
                receipt(&session_id, value, &result.bytes, false)
            }
            operation => {
                let (id, model, expected, native_op) = match operation {
                    Operation::Snapshot {
                        session_id,
                        model_id,
                        expected_history,
                    } => (
                        session_id,
                        model_id,
                        expected_history,
                        NativeProfileOperationV1::Snapshot,
                    ),
                    Operation::State {
                        session_id,
                        model_id,
                        expected_history,
                    } => (
                        session_id,
                        model_id,
                        expected_history,
                        NativeProfileOperationV1::State,
                    ),
                    Operation::Checkpoint {
                        session_id,
                        model_id,
                        expected_history,
                    } => (
                        session_id,
                        model_id,
                        expected_history,
                        NativeProfileOperationV1::Checkpoint,
                    ),
                    Operation::Close {
                        session_id,
                        model_id,
                        expected_history,
                    } => (
                        session_id,
                        model_id,
                        expected_history,
                        NativeProfileOperationV1::Close,
                    ),
                    Operation::Scene {
                        session_id,
                        model_id,
                        expected_history,
                        options,
                        label_policy,
                        grid_ink,
                        cached,
                    } => (
                        session_id,
                        model_id,
                        expected_history,
                        NativeProfileOperationV1::RuntimeScene {
                            options,
                            label_policy,
                            grid_ink,
                            cached,
                        },
                    ),
                    Operation::Open { .. }
                    | Operation::View { .. }
                    | Operation::FeatureCodes { .. }
                    | Operation::EngineeringAttributes { .. }
                    | Operation::GisProjection { .. }
                    | Operation::Properties { .. }
                    | Operation::Pick { .. }
                    | Operation::SelectionRange { .. }
                    | Operation::PackageCheckpoint { .. }
                    | Operation::EditMode { .. }
                    | Operation::Control { .. }
                    | Operation::SaveControl { .. }
                    | Operation::Discard { .. } => unreachable!(),
                };
                let session = owned(&mut held, &scope, &id, &model, &expected)?;
                if matches!(&native_op, NativeProfileOperationV1::Close) {
                    if session.saving {
                        return Err(Failure::conflict(
                            "profile_save_in_progress",
                            "Finish the admitted Save before closing this Profile",
                        ));
                    }
                    let current = Store::open(database)
                        .map_err(source_error)?
                        .profile_recovery(&source_scope(&scope), &id)
                        .map_err(source_error)?;
                    if current.as_ref().map(|recovery| &recovery.source) != Some(&session.source)
                        || current.as_ref().map(|recovery| {
                            recovery
                                .draft
                                .as_ref()
                                .map_or(&recovery.source.initial_history, |(draft, _)| {
                                    &draft.history
                                })
                        }) != Some(&session.retained_history)
                    {
                        return Err(Failure::conflict(
                            "core_profile_source_changed",
                            "Exact original changed before native Close",
                        ));
                    }
                    authorized()?;
                    if session.has_draft {
                        controls::retain_checkpoint(session, database, authorized)?;
                    }
                }
                let result = session
                    .session
                    .request_at(native_op, Some(expected), DEADLINE, &|| false)
                    .map_err(native)?;
                authorized()?;
                session.touched = Instant::now();
                let closed = result.reply.closed;
                let receipt = receipt(&id, session, &result.bytes, closed)?;
                if closed {
                    // Closing releases only the worker. Authored recovery stays
                    // durable until explicit discard or a verified saved successor.
                    if session.retained_history == session.source.initial_history {
                        Store::open(database)
                            .map_err(source_error)?
                            .remove_profile_recovery(
                                &source_scope(&scope),
                                &session.source,
                                &session.retained_history,
                            )
                            .map_err(source_error)?;
                    }
                    authorized()?;
                    held.remove(&id);
                }
                Ok(receipt)
            }
        }?;
        answer["rehydrated"] = json!(rehydrated);
        Ok(answer)
    }
    #[cfg(test)]
    pub fn expire_for_test(&self, id: &str) {
        if let Some(value) = self.held.lock().unwrap().get_mut(id) {
            value.touched = Instant::now() - IDLE;
        }
    }
    #[cfg(test)]
    pub fn held_count_for_test(&self) -> usize {
        self.held.lock().unwrap().len()
    }
}
fn validate_label_fields(fields: &[String]) -> Result<(), Failure> {
    if fields.len() > 64
        || fields.iter().any(|field| {
            !matches!(
                field.as_str(),
                "number"
                    | "station"
                    | "type"
                    | "height"
                    | "alignment"
                    | "comment1"
                    | "comment2"
                    | "comment3"
            )
        })
    {
        return Err(invalid(
            "Use at most 64 existing StructureLabelField values",
        ));
    }
    Ok(())
}
fn read_capability(operation: &Operation) -> Option<NetworkCapability> {
    match operation {
        Operation::PackageCheckpoint { .. } => Some(NetworkCapability::ProfilePackageCheckpointV1),
        Operation::EditMode { .. }
        | Operation::Control { .. }
        | Operation::SaveControl { .. }
        | Operation::Discard { .. } => Some(NetworkCapability::ProfileEditControlsV1),
        Operation::View { observe: true, .. } => Some(NetworkCapability::ProfileObserveV1),
        Operation::View { .. } => Some(NetworkCapability::ProfileViewV1),
        Operation::Properties { .. } => Some(NetworkCapability::ProfilePropertiesV1),
        Operation::Pick { .. } | Operation::SelectionRange { .. } => {
            Some(NetworkCapability::ProfileSelectionV1)
        }
        Operation::FeatureCodes { .. }
        | Operation::EngineeringAttributes { .. }
        | Operation::GisProjection { .. } => Some(NetworkCapability::ProfileReadProjectionsV1),
        _ => None,
    }
}
fn require_read_capability(
    capabilities: &[NetworkCapability],
    required: NetworkCapability,
) -> Result<(), Failure> {
    if !capabilities.contains(&required) {
        let token: String = required.into();
        return Err(Failure::unavailable("core_profile_native_refused",
            format!("The captured Network worker has no {token} capability"))
            .remedy("Install an admitted compatible same-lane Network component and explicitly reopen the exact original; no Core/browser projection fallback exists")
            .detail(json!({"native_code":"network_engine_incompatible","required_capability":token})));
    }
    Ok(())
}
fn validate_selection_payload(
    operation: &Operation,
    bytes: &[u8],
    revision: &Value,
) -> Result<(), Failure> {
    let (axis, pick) = match operation {
        Operation::Pick { request, .. } => (request.axis_pin_digest.as_str(), true),
        Operation::SelectionRange { request, .. } => (request.axis_pin_digest.as_str(), false),
        _ => return Ok(()),
    };
    let value: Value = serde_json::from_slice(bytes).map_err(invalid)?;
    if &value["model_revision"] != revision
        || value["axis_pin_digest"] != axis
        || if pick {
            !value["candidates"].is_array()
        } else {
            !value["entity_ids"]
                .as_array()
                .is_some_and(|v| v.iter().all(Value::is_string))
        }
    {
        return Err(invalid(
            "Native Profile selection crossed its retained revision/axis or result shape",
        ));
    }
    Ok(())
}
fn validate_view_payload(
    bytes: &[u8],
    model_id: &str,
    history: &Value,
    retained_asset_count: usize,
    expected: &ds_command_kernel::profile_display::DisplayState,
) -> Result<ds_command_kernel::profile_display::DisplayState, Failure> {
    let projection: Value = serde_json::from_slice(bytes).map_err(invalid)?;
    let display: ds_command_kernel::profile_display::DisplayState =
        serde_json::from_value(projection["display_state"].clone()).map_err(invalid)?;
    if projection["model_id"] != model_id
        || &projection["history"] != history
        || projection["model_revision"] != history["model_revision"]
        || projection["retained_asset_count"] != retained_asset_count
        || &display != expected
    {
        return Err(invalid(
            "Native view differs from retained model, history, assets or requested display state",
        ));
    }
    Ok(display)
}
fn validate_properties_payload(
    bytes: &[u8],
    entity_id: &str,
    revision: &Value,
) -> Result<(), Failure> {
    let sheet: Value = serde_json::from_slice(bytes).map_err(invalid)?;
    if sheet["schema_version"] != 1
        || sheet["entity_id"] != entity_id
        || &sheet["revision"] != revision
    {
        return Err(invalid(
            "Native Properties differ from retained entity/revision/schema",
        ));
    }
    Ok(())
}
fn read_projection_operation(
    operation: &Operation,
    coordinate_system: &str,
) -> Result<Option<NativeProfileOperationV1>, Failure> {
    match operation {
        Operation::Pick {
            request,
            expected_history,
            ..
        } => {
            if expected_history["model_revision"].as_str() != Some(request.model_revision.as_str())
                || request.axis_pin_digest.is_empty()
                || !request.scene_x.is_finite()
                || !request.scene_y.is_finite()
                || !request.tolerance_scene_units.is_finite()
                || request.tolerance_scene_units < 0.
                || !request.scene_units_per_px.is_finite()
                || request.scene_units_per_px <= 0.
            {
                return Err(invalid(
                    "Use retained revision/axis and finite Profile pick geometry",
                ));
            }
        }
        Operation::SelectionRange {
            request,
            expected_history,
            ..
        } => {
            if expected_history["model_revision"].as_str() != Some(request.model_revision.as_str())
                || request.axis_pin_digest.is_empty()
                || request.from_entity_id.is_empty()
                || request.to_entity_id.is_empty()
            {
                return Err(invalid(
                    "Use retained revision/axis and exact range endpoints",
                ));
            }
        }
        _ => {}
    }
    Ok(match operation {
        Operation::FeatureCodes { .. } => Some(NativeProfileOperationV1::FeatureCodes {}),
        Operation::Pick { request,.. } => Some(NativeProfileOperationV1::Pick{request:request.clone()}),
        Operation::SelectionRange { request,.. } => Some(NativeProfileOperationV1::SelectionRange{request:request.clone()}),
        Operation::Properties { entity_id, .. } => Some(
            serde_json::from_value(json!({"action":"properties","entity_id":entity_id}))
                .map_err(|_| invalid("Use the exact canonical entity ID (1..128 UTF-8 bytes, no controls or edge whitespace)"))?,
        ),
        Operation::EngineeringAttributes { table, .. } => {
            Some(NativeProfileOperationV1::EngineeringAttributes {
                table: table.clone(),
            })
        }
        Operation::GisProjection { label_fields, .. } => {
            validate_label_fields(label_fields)?;
            Some(NativeProfileOperationV1::GisProjection {
                coordinate_system: coordinate_system.into(),
                label_fields: serde_json::from_value(json!(label_fields)).map_err(invalid)?,
            })
        }
        _ => None,
    })
}
fn owned<'a>(
    held: &'a mut BTreeMap<String, Held>,
    scope: &Scope,
    id: &str,
    model: &str,
    expected: &Value,
) -> Result<&'a mut Held, Failure> {
    let value = held
        .get_mut(id)
        .filter(|v| &v.scope == scope && v.session.model_id() == model)
        .ok_or_else(|| {
            Failure::unavailable(
                "core_profile_session_unavailable",
                "No retained Profile belongs to this exact account/lane/project/model",
            )
            .remedy("Explicitly reopen the exact package through the same Core owner")
        })?;
    if value.session.history() != expected {
        return Err(Failure::conflict(
            "core_profile_history_conflict",
            "Profile history changed before the request",
        )
        .remedy("Read the retained session head and submit the intended operation again"));
    }
    Ok(value)
}
fn receipt(id: &str, held: &Held, bytes: &[u8], closed: bool) -> Result<Value, Failure> {
    if bytes.len() > 64 * 1024 * 1024 {
        return Err(invalid(
            "Profile artifact exceeds the 64 MiB host response bound",
        ));
    }
    // Exact native JSON strings inside snapshot/checkpoint stay opaque text.
    let mut payload: Value = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(bytes)
            .map_err(|_| invalid("Native Profile artifact is not its declared JSON"))?
    };
    if payload["table_counts"].is_object() && payload["revision"].is_object() {
        if payload["model_revision"].is_null() {
            payload["model_revision"] = json!(held.source_revision);
        }
        if payload["coordinate_system"].is_null() {
            payload["coordinate_system"] = json!(held.coordinate_system);
        }
    }
    if payload["presentation"]["history"].is_object()
        && payload["presentation"]["revision"].is_object()
    {
        if payload["presentation"]["model_revision"].is_null() {
            payload["presentation"]["model_revision"] = json!(held.source_revision);
        }
        if payload["presentation"]["coordinate_system"].is_null() {
            payload["presentation"]["coordinate_system"] = json!(held.coordinate_system);
        }
    }
    Ok(
        json!({"schema":RECEIPT_SCHEMA,"session_id":id,"project":held.scope.project,
        "lane":held.scope.lane,"package_sha256":held.package_sha256,"source_revision":held.source_revision,"coordinate_system":held.coordinate_system,
        "model_id":held.session.model_id(),"history":held.session.history(),
        "engine":held.session.pin(),"closed":closed,"payload":payload}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn closed_schema_refuses_arbitrary_actions_and_extra_fields() {
        for bytes in [br#"{"schema":"ds.core.profile-session/v1","operation":{"action":"run","argv":[]}}"#.as_slice(),
            br#"{"schema":"wrong","operation":{"action":"open","package_base64":"AA==","package_sha256":"a"}}"#,
            br#"{"schema":"ds.core.profile-session/v1","operation":{"action":"open","package_base64":"AA==","package_sha256":"a","path":"/etc/passwd"}}"#] {
            assert!(Request::parse(bytes).is_err());
        }
    }
    #[test]
    fn bulk_update_refusal_preserves_plain_message_and_structured_requirement() {
        let message = "Update Network to edit this terrain batch.";
        let requirement =
            json!({"component":"network", "required_capability":"profile_bulk_controls_v1"});
        let error = native(NetworkFailure {
            code: "network_engine_incompatible",
            message: message.into(),
            owner_error: None,
            owner_code: None,
            owner_detail: Some(Box::new(requirement.clone())),
            owner_remedy: Some(message.into()),
            owner_invalid_input: None,
        });
        assert_eq!(error.code(), "core_profile_native_refused");
        assert_eq!(error.message(), message);
        let detail = error.detail_value().unwrap();
        assert_eq!(detail["native_code"], "network_engine_incompatible");
        assert_eq!(detail["native_detail"], requirement);
        assert_eq!(detail["native_remedy"], message);
    }
    #[test]
    fn only_typed_controls_admit_bulk_requests_up_to_eight_mib() {
        let mut value = json!({"schema":REQUEST_SCHEMA,"operation":{
        "action":"control","session_id":"captured","model_id":"model",
        "expected_history":{},"operation":{"action":"preview_transaction","commands":[{
            "command_id":"one","command":{"owner_payload":"x".repeat(70*1024)}
        }]}}});
        let bytes = serde_json::to_vec(&value).unwrap();
        assert!(bytes.len() > 64 * 1024);
        assert!(Request::parse(&bytes).is_ok());
        value["operation"]["operation"]["commands"][0]["command"]["owner_payload"] =
            json!("x".repeat(ds_network_contracts::profile::MAX_PROFILE_CONTROL_INPUT_BYTES));
        assert!(Request::parse(&serde_json::to_vec(&value).unwrap()).is_err());
        let state = json!({"schema":REQUEST_SCHEMA,"operation":{
            "action":"state","session_id":"captured","model_id":"model",
            "expected_history":{"large":"x".repeat(70*1024)}}});
        assert!(Request::parse(&serde_json::to_vec(&state).unwrap()).is_err());
        value["operation"]["operation"]["action"] = json!("unknown");
        assert!(Request::parse(&serde_json::to_vec(&value).unwrap()).is_err());
    }
    #[test]
    fn gis_labels_are_closed_and_bounded() {
        assert!(
            validate_label_fields(&["station".into(), "number".into(), "number".into()]).is_ok()
        );
        assert!(validate_label_fields(&["arbitrary".into()]).is_err());
        assert!(validate_label_fields(&vec!["number".into(); 65]).is_err());
    }
    fn style_scope() -> Scope {
        Scope {
            principal: "fixture".into(),
            lane: "canary".into(),
            deployment: "fixture-deployment".into(),
            install: "fixture-install".into(),
            project: "fixture-project".into(),
        }
    }
    fn style_owner() -> ds_cli_auth::ProviderIdentity {
        ds_cli_auth::ProviderIdentity::new("canary", &"a".repeat(64), "fixture").unwrap()
    }
    fn authored_style() -> Value {
        serde_json::from_str(include_str!(
            "../../../../ds-command-kernel/tests/fixtures/profile-display-style.json"
        ))
        .unwrap()
    }
    fn style_catalogue(document: Value) -> StyleCatalogue {
        StyleCatalogue {
            identity: style_owner(),
            lane: "canary".into(),
            project: "fixture-project".into(),
            document: json!({"project_id":"fixture-project","styles":{"ds_grid/profile":document}}),
        }
    }
    #[test]
    fn complete_view_requires_its_capability_and_exact_host_adoption_fences() {
        let history = json!({"model_revision":"revision"});
        let operation = Operation::View {
            session_id: "held".into(),
            model_id: "model".into(),
            expected_history: history.clone(),
            options: json!({}),
            patch: json!({}),
            style_document: Value::Null,
            cached: true,
            observe: false,
        };
        assert_eq!(
            read_capability(&operation),
            Some(NetworkCapability::ProfileViewV1)
        );
        assert!(
            require_read_capability(
                &[NetworkCapability::ProfileRuntimeV1],
                NetworkCapability::ProfileViewV1
            )
            .is_err()
        );
        let display: ds_command_kernel::profile_display::DisplayState = Default::default();
        let good = json!({"model_id":"model","model_revision":"revision","history":history,
            "retained_asset_count":2,"display_state":display});
        assert_eq!(
            validate_view_payload(
                &serde_json::to_vec(&good).unwrap(),
                "model",
                &history,
                2,
                &display
            )
            .unwrap(),
            display
        );
        for key in [
            "model_id",
            "model_revision",
            "history",
            "retained_asset_count",
            "display_state",
        ] {
            let mut bad = good.clone();
            bad[key] = Value::Null;
            assert!(
                validate_view_payload(
                    &serde_json::to_vec(&bad).unwrap(),
                    "model",
                    &history,
                    2,
                    &display
                )
                .is_err(),
                "{key}"
            );
        }
        let mut bad = good;
        bad["display_state"]["vertical_exaggeration"] = json!(6.);
        assert!(
            validate_view_payload(
                &serde_json::to_vec(&bad).unwrap(),
                "model",
                &history,
                2,
                &display
            )
            .is_err()
        );
    }
    #[test]
    fn view_preparation_refusal_preserves_public_invalid_request_code() {
        let error = |owner: &str| NetworkFailure {
            code: "network_engine_refused",
            message: "invalid authored view".into(),
            owner_error: None,
            owner_code: Some(owner.into()),
            owner_detail: None,
            owner_remedy: None,
            owner_invalid_input: None,
        };
        assert_eq!(
            view_native(error("profile_view_invalid")).code(),
            "core_profile_request_invalid"
        );
        assert_eq!(
            view_native(error("profile_scene_unavailable")).code(),
            "core_profile_native_refused"
        );
        assert_eq!(
            native(error("profile_view_invalid")).code(),
            "core_profile_native_refused"
        );
    }
    fn physical_style(document: Value) -> Result<(), String> {
        ds_command_kernel::profile_encoding::resolve(&Default::default(), &document)?;
        ds_command_kernel::profile_encoding::review_status_colors(&document)?;
        ds_command_kernel::profile_encoding::review_box_ink(&document)?;
        Ok(())
    }
    #[test]
    fn present_style_never_fetches_or_replaces_malformed_authored_input() {
        for document in [
            json!({}),
            json!({"metadata":{"profile_pens":{"grid":{"width_mm":-1}}}}),
        ] {
            let (actual, capture) = resolve_style(
                &style_scope(),
                document.clone(),
                None,
                &|| panic!("Present style does not acquire authority again"),
                &|| panic!("No native identity read for caller document"),
                &|| panic!("Malformed supplied style is never replaced"),
            )
            .unwrap();
            assert_eq!(actual, document);
            assert!(capture.is_none());
            assert!(physical_style(actual).is_err());
        }
    }
    #[test]
    fn null_style_captures_exact_authored_document_and_repeats_without_fetch() {
        let calls = std::cell::Cell::new(0);
        let mut document = authored_style();
        document["metadata"]["owner_extension"] = json!({"untouched":[1,"exact",false]});
        let fetch = || {
            calls.set(calls.get() + 1);
            Ok(style_catalogue(document.clone()))
        };
        let (actual, candidate) = resolve_style(
            &style_scope(),
            Value::Null,
            None,
            &|| Ok(()),
            &|| Ok(style_owner()),
            &fetch,
        )
        .unwrap();
        assert_eq!(actual, document);
        physical_style(actual.clone()).unwrap(); // Unchanged complete 17-pen/physical owner.
        let mut captured = None;
        adopt_style_capture(&mut captured, candidate, &|| Ok(()), &|| Ok(style_owner())).unwrap();
        let (repeated, _) = resolve_style(
            &style_scope(),
            Value::Null,
            captured.as_ref(),
            &|| Ok(()),
            &|| Ok(style_owner()),
            &fetch,
        )
        .unwrap();
        assert_eq!(repeated, document);
        assert_eq!(calls.get(), 1);
    }
    #[test]
    fn missing_excessive_and_foreign_catalogues_are_not_adopted() {
        let mut candidates = vec![
            style_catalogue(Value::Null),
            style_catalogue(json!({"padding":"x".repeat(MAX_STYLE_BYTES)})),
        ];
        let mut lane = style_catalogue(authored_style());
        lane.lane = "stable".into();
        candidates.push(lane);
        let mut project = style_catalogue(authored_style());
        project.project = "foreign".into();
        candidates.push(project);
        let mut body = style_catalogue(authored_style());
        body.document["project_id"] = json!("foreign");
        candidates.push(body);
        for catalogue in candidates {
            let fetch = || {
                Ok(StyleCatalogue {
                    identity: catalogue.identity.clone(),
                    lane: catalogue.lane.clone(),
                    project: catalogue.project.clone(),
                    document: catalogue.document.clone(),
                })
            };
            assert!(
                resolve_style(
                    &style_scope(),
                    Value::Null,
                    None,
                    &|| Ok(()),
                    &|| Ok(style_owner()),
                    &fetch
                )
                .is_err()
            );
        }
    }
    #[test]
    fn delayed_style_read_refuses_changed_uid_lane_or_audience() {
        for changed in [
            ds_cli_auth::ProviderIdentity::new("canary", &"a".repeat(64), "foreign").unwrap(),
            ds_cli_auth::ProviderIdentity::new("stable", &"a".repeat(64), "fixture").unwrap(),
            ds_cli_auth::ProviderIdentity::new("canary", &"b".repeat(64), "fixture").unwrap(),
        ] {
            let current = std::cell::RefCell::new(style_owner());
            let fetch = || {
                *current.borrow_mut() = changed.clone();
                Ok(style_catalogue(authored_style()))
            };
            let error = resolve_style(
                &style_scope(),
                Value::Null,
                None,
                &|| Ok(()),
                &|| Ok(current.borrow().clone()),
                &fetch,
            )
            .err()
            .unwrap();
            assert_eq!(error.code(), "core_profile_styles_unavailable");
        }
    }
    #[test]
    fn missing_or_late_native_authority_never_adopts_a_style() {
        let refused = || {
            Err(Failure::unauthorized(
                "server_owner_changed",
                "fixture authority changed",
            ))
        };
        assert!(
            resolve_style(
                &style_scope(),
                Value::Null,
                None,
                &refused,
                &|| Ok(style_owner()),
                &|| panic!("Signed-out owner must not fetch")
            )
            .is_err()
        );
        let calls = std::cell::Cell::new(0);
        let authorized = || {
            calls.set(calls.get() + 1);
            if calls.get() > 1 { refused() } else { Ok(()) }
        };
        assert!(
            resolve_style(
                &style_scope(),
                Value::Null,
                None,
                &authorized,
                &|| Ok(style_owner()),
                &|| Ok(style_catalogue(authored_style()))
            )
            .is_err()
        );
        let (_, capture) = resolve_style(
            &style_scope(),
            Value::Null,
            None,
            &|| Ok(()),
            &|| Ok(style_owner()),
            &|| Ok(style_catalogue(authored_style())),
        )
        .unwrap();
        let mut adopted = None;
        assert!(
            adopt_style_capture(&mut adopted, capture.clone(), &refused, &|| Ok(
                style_owner()
            ))
            .is_err()
        );
        assert!(adopted.is_none());
        assert!(
            adopt_style_capture(&mut adopted, capture.clone(), &|| Ok(()), &|| {
                ds_cli_auth::ProviderIdentity::new("canary", &"b".repeat(64), "fixture")
            })
            .is_err()
        );
        assert!(
            resolve_style(
                &style_scope(),
                Value::Null,
                capture.as_ref(),
                &|| Ok(()),
                &|| ds_cli_auth::ProviderIdentity::new("canary", &"b".repeat(64), "fixture"),
                &|| panic!("Changed cached audience must not silently refetch")
            )
            .is_err()
        );
    }
    #[test]
    fn simultaneous_intents_coalesce_only_after_physical_and_late_authority_admission() {
        let cache = std::sync::Mutex::new(None::<StyleCapture>);
        let calls = std::sync::atomic::AtomicUsize::new(0);
        std::thread::scope(|threads| {
            for _ in 0..2 {
                threads.spawn(|| {
                    // Production Holds this same session-owner mutex through View.
                    let mut cached = cache.lock().unwrap();
                    let (document, candidate) = resolve_style(
                        &style_scope(),
                        Value::Null,
                        cached.as_ref(),
                        &|| Ok(()),
                        &|| Ok(style_owner()),
                        &|| {
                            calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                            std::thread::sleep(Duration::from_millis(5));
                            Ok(style_catalogue(authored_style()))
                        },
                    )
                    .unwrap();
                    physical_style(document).unwrap();
                    adopt_style_capture(&mut cached, candidate, &|| Ok(()), &|| Ok(style_owner()))
                        .unwrap();
                });
            }
        });
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1);
    }
    #[test]
    fn inner_catalogue_owner_aba_is_refused_even_when_ambient_returns_to_a() {
        let (_, capture) = resolve_style(
            &style_scope(),
            Value::Null,
            None,
            &|| Ok(()),
            &|| Ok(style_owner()),
            &|| {
                let mut catalogue = style_catalogue(authored_style());
                catalogue.identity =
                    ds_cli_auth::ProviderIdentity::new("canary", &"b".repeat(64), "fixture")
                        .unwrap();
                Ok(catalogue)
            },
        )
        .map(|result| (true, result.1))
        .unwrap_or((false, None));
        assert!(capture.is_none());
        for foreign in [
            ds_cli_auth::ProviderIdentity::new("canary", &"b".repeat(64), "fixture").unwrap(),
            ds_cli_auth::ProviderIdentity::new("canary", &"a".repeat(64), "foreign").unwrap(),
        ] {
            let error = resolve_style(
                &style_scope(),
                Value::Null,
                None,
                &|| Ok(()),
                &|| Ok(style_owner()),
                &|| {
                    let mut catalogue = style_catalogue(authored_style());
                    catalogue.identity = foreign.clone();
                    Ok(catalogue)
                },
            )
            .err()
            .unwrap();
            assert_eq!(error.code(), "core_profile_styles_unavailable");
        }
    }
    #[test]
    fn successful_supplied_successor_clears_cache_failed_physical_input_preserves_it() {
        let (_, mut cache) = resolve_style(
            &style_scope(),
            Value::Null,
            None,
            &|| Ok(()),
            &|| Ok(style_owner()),
            &|| Ok(style_catalogue(authored_style())),
        )
        .unwrap();
        let prior = cache.as_ref().unwrap().document.clone();
        let (bad, _) = resolve_style(
            &style_scope(),
            json!({}),
            cache.as_ref(),
            &|| panic!(),
            &|| panic!(),
            &|| panic!(),
        )
        .unwrap();
        assert!(physical_style(bad).is_err());
        assert_eq!(cache.as_ref().unwrap().document, prior);
        let (valid, successor) = resolve_style(
            &style_scope(),
            authored_style(),
            cache.as_ref(),
            &|| panic!(),
            &|| panic!(),
            &|| panic!(),
        )
        .unwrap();
        physical_style(valid).unwrap();
        adopt_style_capture(&mut cache, successor, &|| Ok(()), &|| {
            panic!("Supplied style has no acquired identity")
        })
        .unwrap();
        assert!(cache.is_none());
        let reads = std::cell::Cell::new(0);
        resolve_style(
            &style_scope(),
            Value::Null,
            cache.as_ref(),
            &|| Ok(()),
            &|| Ok(style_owner()),
            &|| {
                reads.set(reads.get() + 1);
                Ok(style_catalogue(authored_style()))
            },
        )
        .unwrap();
        assert_eq!(reads.get(), 1);
    }
    #[test]
    fn native_scope_mismatch_or_fetch_refusal_does_not_use_another_style() {
        let mut scope = style_scope();
        scope.principal = "foreign".into();
        assert!(
            resolve_style(
                &scope,
                Value::Null,
                None,
                &|| Ok(()),
                &|| Ok(style_owner()),
                &|| panic!("Foreign native principal must not fetch")
            )
            .is_err()
        );
        let mut scope = style_scope();
        scope.lane = "stable".into();
        assert!(
            resolve_style(
                &scope,
                Value::Null,
                None,
                &|| Ok(()),
                &|| Ok(style_owner()),
                &|| panic!("Foreign native lane must not fetch")
            )
            .is_err()
        );
        let error = resolve_style(
            &style_scope(),
            Value::Null,
            None,
            &|| Ok(()),
            &|| Ok(style_owner()),
            &|| Err(style_error("Authored catalogue unavailable")),
        )
        .err()
        .unwrap();
        assert_eq!(error.code(), "core_profile_styles_unavailable");
    }
    #[test]
    fn package_digest_is_exact_before_decode_or_process() {
        let bytes = b"synthetic package bytes";
        assert_eq!(
            decode_package(&STANDARD.encode(bytes), &sha(bytes)).unwrap(),
            bytes
        );
        assert!(decode_package(&STANDARD.encode(bytes), &"0".repeat(64)).is_err());
        assert!(decode_package("!", &"0".repeat(64)).is_err());
    }
    #[test]
    fn read_projection_actions_are_closed_typed_and_preserve_ordered_label_fields() {
        let base = |action: &str| json!({"schema":REQUEST_SCHEMA,"operation":{"action":action,"session_id":"captured","model_id":"fixture-model","expected_history":{"model_revision":"head"}}});
        let feature = Request::parse(&serde_json::to_vec(&base("feature_codes")).unwrap()).unwrap();
        assert!(matches!(
            read_projection_operation(&feature.operation, "EPSG:32735").unwrap(),
            Some(NativeProfileOperationV1::FeatureCodes { .. })
        ));
        for table in ["structures", "sections"] {
            let mut request = base("engineering_attributes");
            request["operation"]["table"] = json!(table);
            let parsed = Request::parse(&serde_json::to_vec(&request).unwrap()).unwrap();
            let forwarded = serde_json::to_value(
                read_projection_operation(&parsed.operation, "EPSG:32735")
                    .unwrap()
                    .unwrap(),
            )
            .unwrap();
            assert_eq!(
                forwarded,
                json!({"action":"engineering_attributes","table":table})
            );
        }
        for table in ["unknown", "structure", "Structures", "", "terrain"] {
            let mut request = base("engineering_attributes");
            request["operation"]["table"] = json!(table);
            assert!(Request::parse(&serde_json::to_vec(&request).unwrap()).is_err());
        }
        let mut feature_extra = base("feature_codes");
        feature_extra["operation"]["table"] = json!("structures");
        assert!(Request::parse(&serde_json::to_vec(&feature_extra).unwrap()).is_err());
        let mut gis = base("gis_projection");
        gis["operation"]["label_fields"] = json!(["station", "number", "number"]);
        let parsed = Request::parse(&serde_json::to_vec(&gis).unwrap()).unwrap();
        assert_eq!(
            serde_json::to_value(
                read_projection_operation(&parsed.operation, "EPSG:32735")
                    .unwrap()
                    .unwrap()
            )
            .unwrap(),
            json!({"action":"gis_projection","coordinate_system":"EPSG:32735","label_fields":["station","number","number"]})
        );
        let mut caller_crs = gis.clone();
        caller_crs["operation"]["coordinate_system"] = json!("EPSG:4326");
        assert!(Request::parse(&serde_json::to_vec(&caller_crs).unwrap()).is_err());
        gis["operation"]["label_fields"] = json!(["arbitrary"]);
        let parsed = Request::parse(&serde_json::to_vec(&gis).unwrap()).unwrap();
        assert!(read_projection_operation(&parsed.operation, "EPSG:32735").is_err());
    }
    #[test]
    fn old_or_unrelated_engine_capabilities_refuse_the_projection_boundary() {
        for capabilities in [
            vec![],
            vec![
                NetworkCapability::ProfileSessionV1,
                NetworkCapability::ProfileRuntimeV1,
            ],
            vec![NetworkCapability::Unknown("unrelated".into())],
        ] {
            let error =
                require_read_capability(&capabilities, NetworkCapability::ProfileReadProjectionsV1)
                    .unwrap_err();
            assert_eq!(error.code(), "core_profile_native_refused");
            assert_eq!(
                error.detail_value().unwrap()["native_code"],
                "network_engine_incompatible"
            );
        }
        require_read_capability(
            &[NetworkCapability::ProfileReadProjectionsV1],
            NetworkCapability::ProfileReadProjectionsV1,
        )
        .unwrap();
    }
    #[test]
    fn properties_is_closed_canonical_and_requires_its_distinct_capability() {
        let base = |entity: Value| json!({"schema":REQUEST_SCHEMA,"operation":{"action":"properties","session_id":"captured","model_id":"fixture-model","expected_history":{"model_revision":"head"},"entity_id":entity}});
        let request = Request::parse(&serde_json::to_vec(&base(json!("str-1"))).unwrap()).unwrap();
        assert_eq!(
            serde_json::to_value(
                read_projection_operation(&request.operation, "ignored")
                    .unwrap()
                    .unwrap()
            )
            .unwrap(),
            json!({"action":"properties","entity_id":"str-1"})
        );
        assert_eq!(
            read_capability(&request.operation),
            Some(NetworkCapability::ProfilePropertiesV1)
        );
        for id in [
            "".to_owned(),
            " str-1".into(),
            "str-1 ".into(),
            "str\n1".into(),
            "a".repeat(129),
            "é".repeat(65),
        ] {
            let parsed = Request::parse(&serde_json::to_vec(&base(json!(id))).unwrap()).unwrap();
            assert!(read_projection_operation(&parsed.operation, "").is_err());
        }
        let mut extra = base(json!("str-1"));
        extra["operation"]["mutate"] = json!(true);
        assert!(Request::parse(&serde_json::to_vec(&extra).unwrap()).is_err());
        assert!(Request::parse(&serde_json::to_vec(&base(json!(1))).unwrap()).is_err());
        for caps in [vec![], vec![NetworkCapability::ProfileReadProjectionsV1]] {
            let error =
                require_read_capability(&caps, NetworkCapability::ProfilePropertiesV1).unwrap_err();
            assert_eq!(
                error.detail_value().unwrap()["required_capability"],
                "profile_properties_v1"
            );
        }
        require_read_capability(
            &[NetworkCapability::ProfilePropertiesV1],
            NetworkCapability::ProfilePropertiesV1,
        )
        .unwrap();
    }
    #[test]
    fn invalid_properties_entity_refuses_before_store_or_worker_lookup() {
        let sessions = Sessions::default();
        let error = sessions
            .execute(
                style_scope(),
                Path::new("/absent-properties-fixture"),
                Path::new("/absent-properties-fixture/store.sqlite"),
                Request {
                    schema: REQUEST_SCHEMA.into(),
                    operation: Operation::Properties {
                        session_id: "not-a-session".into(),
                        model_id: "fixture-model".into(),
                        expected_history: json!({}),
                        entity_id: " invalid".into(),
                    },
                },
                &|| Ok(()),
            )
            .unwrap_err();
        assert_eq!(error.code(), "core_profile_request_invalid");
        assert_eq!(sessions.held_count_for_test(), 0);
    }
    #[test]
    fn properties_reject_crossed_native_entity_revision_and_schema_before_adoption() {
        let good = json!({"schema_version":1,"entity_id":"str-1","revision":"head","groups":[]});
        validate_properties_payload(&serde_json::to_vec(&good).unwrap(), "str-1", &json!("head"))
            .unwrap();
        for (field, value) in [
            ("schema_version", json!(2)),
            ("entity_id", json!("other")),
            ("revision", json!("old")),
        ] {
            let mut bad = good.clone();
            bad[field] = value;
            assert!(
                validate_properties_payload(
                    &serde_json::to_vec(&bad).unwrap(),
                    "str-1",
                    &json!("head")
                )
                .is_err()
            );
        }
    }
    #[test]
    fn unknown_or_foreign_handles_never_launch_a_process() {
        let sessions = Sessions::default();
        let scope = Scope {
            principal: "fixture".into(),
            lane: "canary".into(),
            deployment: "https://fixture.invalid".into(),
            install: "fixture-install".into(),
            project: "fixture-project".into(),
        };
        let request = Request {
            schema: REQUEST_SCHEMA.into(),
            operation: Operation::Snapshot {
                session_id: "not-a-session".into(),
                model_id: "fixture-model".into(),
                expected_history: Value::Null,
            },
        };
        let refused = sessions
            .execute(
                scope,
                Path::new("/absent"),
                Path::new("/absent/store.sqlite"),
                request,
                &|| Ok(()),
            )
            .unwrap_err();
        assert_eq!(refused.code(), "core_profile_session_unavailable");
    }
    #[test]
    fn selection_is_closed_distinct_cap_and_preserves_exact_requests() {
        let pick = json!({"model_revision":"head","axis_pin_digest":"axis","scene_x":1.,"scene_y":2.,"tolerance_scene_units":3.,"scene_units_per_px":1.,"visibility":{"showGround":true,"showTerrainPoints":true,"showStructures":true,"showWire":false,"showAttachments":false}});
        let range = json!({"model_revision":"head","axis_pin_digest":"axis","from_entity_id":"a","to_entity_id":"b"});
        for (action, query) in [("pick", pick.clone()), ("selection_range", range)] {
            let value = json!({"schema":REQUEST_SCHEMA,"operation":{"action":action,"session_id":"captured","model_id":"model","expected_history":{"model_revision":"head"},"request":query}});
            let parsed = Request::parse(&serde_json::to_vec(&value).unwrap()).unwrap();
            assert_eq!(
                read_capability(&parsed.operation),
                Some(NetworkCapability::ProfileSelectionV1)
            );
            assert_eq!(
                serde_json::to_value(
                    read_projection_operation(&parsed.operation, "")
                        .unwrap()
                        .unwrap()
                )
                .unwrap(),
                json!({"action":action,"request":query})
            );
            let mut invalid = value;
            invalid["operation"]["request"]["mutate"] = json!(true);
            assert!(Request::parse(&serde_json::to_vec(&invalid).unwrap()).is_err());
        }
        for caps in [
            vec![],
            vec![
                NetworkCapability::ProfileReadProjectionsV1,
                NetworkCapability::ProfilePropertiesV1,
            ],
        ] {
            let error =
                require_read_capability(&caps, NetworkCapability::ProfileSelectionV1).unwrap_err();
            assert_eq!(
                error.detail_value().unwrap()["required_capability"],
                "profile_selection_v1"
            );
        }
        for field in [
            "scene_x",
            "scene_y",
            "tolerance_scene_units",
            "scene_units_per_px",
        ] {
            let mut invalid = pick.clone();
            invalid[field] = Value::Null;
            assert!(Request::parse(&serde_json::to_vec(&json!({"schema":REQUEST_SCHEMA,"operation":{"action":"pick","session_id":"captured","model_id":"model","expected_history":{"model_revision":"head"},"request":invalid}})).unwrap()).is_err());
        }
    }
    #[test]
    fn selection_rejects_crossed_worker_axis_revision_and_result_before_adoption() {
        let operation = Operation::SelectionRange {
            session_id: "captured".into(),
            model_id: "model".into(),
            expected_history: json!({"model_revision":"head"}),
            request: NativeProfileSelectionRangeRequestV1 {
                model_revision: "head".into(),
                axis_pin_digest: "axis".into(),
                from_entity_id: "a".into(),
                to_entity_id: "b".into(),
            },
        };
        let expected =
            json!({"model_revision":"head","axis_pin_digest":"axis","entity_ids":["a","b"]});
        validate_selection_payload(
            &operation,
            &serde_json::to_vec(&expected).unwrap(),
            &json!("head"),
        )
        .unwrap();
        for (field, value) in [
            ("model_revision", json!("other")),
            ("axis_pin_digest", json!("other")),
            ("entity_ids", json!([1])),
        ] {
            let mut invalid = expected.clone();
            invalid[field] = value;
            assert!(
                validate_selection_payload(
                    &operation,
                    &serde_json::to_vec(&invalid).unwrap(),
                    &json!("head")
                )
                .is_err()
            );
        }
    }
}

#[cfg(test)]
#[path = "profile_sessions_package_tests.rs"]
mod package_tests;
