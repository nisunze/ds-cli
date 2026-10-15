//! Process-lifetime Profile ownership. Parsing/engineering stay in their existing
//! Grid exchange and managed Network owners; no browser, SDK or caller argv.
use base64::{Engine, engine::general_purpose::STANDARD};
use ds_cli_contract::Failure;
use ds_grid_exchange::profile_session::ProfileSessionSeed;
use ds_network_client::{CapturedNetworkClient, NetworkFailure, ProfileSession};
use ds_network_contracts::profile::NativeProfileGridInkV1;
use ds_network_contracts::profile::{
    NativeProfileOpenV1, NativeProfileOperationV1, PROFILE_OPEN_SCHEMA,
};
use ds_sync_store::{ProfileSourceHeader, ProfileSourceScope, Store};
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
pub const MAX_PACKAGE: usize = 32 * 1024 * 1024;
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
        if !matches!(request.operation, Operation::Open { .. }) && bytes.len() > 64 * 1024 {
            return Err(invalid("Profile controls exceed 64 KiB"));
        }
        Ok(request)
    }
}
fn invalid(message: impl ToString) -> Failure {
    Failure::invalid("core_profile_request_invalid", message.to_string())
        .remedy("Use core.profile-session contract 1 with an exact package SHA or captured session/model/history")
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
/// Admit the existing package owner, including every member/library pin. This
/// source conversion does not solve/recalculate: the process owns live state.
fn package_open(bytes: &[u8]) -> Result<(NativeProfileOpenV1, u64, String), Failure> {
    let package = ds_grid_exchange::package::unpack(bytes).map_err(|e| invalid(e.to_string()))?;
    let manifest = package.manifest.model;
    let protection = ds_grid_exchange::linked_models::protection_from_assets(
        manifest.model_id.as_str(),
        &package.assets,
    );
    let session = ds_grid_engine::GridSession::open_model(
        package.snapshot,
        manifest.model_id.as_str(),
        protection,
    );
    if !session.is_valid() {
        return Err(invalid("Package cannot establish a valid Profile session"));
    }
    let seed = ProfileSessionSeed::capture(
        &session,
        manifest.model_id,
        manifest.presentation,
        &package.assets,
    );
    let seed_json = serde_json::to_string(&seed)
        .map_err(|_| invalid("Cannot encode the admitted Profile seed"))?;
    Ok((
        NativeProfileOpenV1 {
            schema: PROFILE_OPEN_SCHEMA.into(),
            seed_json,
            binding_history: Some(vec![package.exchange_bindings]),
            calculated: None,
            analysis: None,
        },
        manifest.model_revision,
        manifest.coordinate_system.as_str().to_owned(),
    ))
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
        } => Some((session_id, model_id, expected_history)),
    }
}
fn open_original(
    scope: Scope,
    scratch: &Path,
    bytes: &[u8],
    id: &str,
    authorized: &dyn Fn() -> Result<(), Failure>,
) -> Result<(Held, Vec<u8>), Failure> {
    let (open, source_revision, coordinate_system) = package_open(bytes)?;
    let input =
        serde_json::to_vec(&open).map_err(|_| invalid("Cannot encode native Profile opening"))?;
    let workers = std::thread::available_parallelism()
        .map_or(1, usize::from)
        .min(2);
    let client =
        CapturedNetworkClient::capture_for_lane(&scope.lane, workers, &|| false).map_err(native)?;
    authorized()?;
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
        presentation: serde_json::from_str::<Value>(&open.seed_json)
            .map_err(|_| invalid("Admitted seed changed"))?["presentation"]
            .clone(),
        retained_asset_count: serde_json::from_slice::<Value>(&result.bytes)
            .map_err(|_| invalid("Native snapshot changed"))?["retained_asset_count"]
            .as_u64()
            .ok_or_else(|| invalid("Native snapshot has no asset count"))?
            as usize,
        display_state: Default::default(),
        style: None,
        touched: Instant::now(),
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
    presentation: Value,
    retained_asset_count: usize,
    display_state: ds_command_kernel::profile_display::DisplayState,
    style: Option<StyleCapture>,
    touched: Instant,
    source: ProfileSourceHeader,
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
        let mut held = self.held.lock().map_err(|_| {
            Failure::internal(
                "core_profile_session_unavailable",
                "Profile session ownership is poisoned",
            )
        })?;
        // Dropping a native session aborts/reaps its entire supervised worker.
        held.retain(|_, value| {
            // Original-only expiry may never silently discard authored history.
            value.session.history() != &value.source.initial_history
                || (value.touched.elapsed() < IDLE && !value.session.is_closed())
        });
        let mut rehydrated = false;
        if let Some((id, model, expected)) = operation_owner(&request.operation) {
            if !held.contains_key(id) {
                let source_scope = source_scope(&scope);
                let source = Store::open_read_only(database)
                    .map_err(source_error)?
                    .map(|store| store.profile_source(&source_scope, id))
                    .transpose()
                    .map_err(source_error)?
                    .flatten();
                if let Some((header, bytes)) = source {
                    if header.model_id != model || &header.initial_history != expected {
                        return Err(Failure::conflict(
                            "core_profile_history_conflict",
                            "Retained original differs from the exact requested model/initial history",
                        ));
                    }
                    authorized()?;
                    if matches!(request.operation, Operation::Close { .. }) {
                        Store::open(database)
                            .map_err(source_error)?
                            .remove_profile_source(&source_scope, &header)
                            .map_err(source_error)?;
                        authorized()?;
                        return Ok(closed_source_receipt(&scope, &header));
                    }
                    if held.len() >= MAX_SESSIONS {
                        return Err(Failure::unavailable(
                            "core_profile_capacity",
                            "Core already retains eight Profile sessions",
                        ));
                    }
                    let (value, _) = open_original(scope.clone(), scratch, &bytes, id, authorized)?;
                    if value.source != header {
                        return Err(Failure::conflict(
                            "core_profile_source_changed",
                            "Exact original model/history/CRS/managed engine changed during rebind",
                        ));
                    }
                    authorized()?;
                    Store::open(database)
                        .map_err(source_error)?
                        .touch_profile_source(&source_scope, &header, now_ms())
                        .map_err(source_error)?;
                    authorized()?;
                    held.insert(id.to_owned(), value);
                    rehydrated = true;
                }
            }
        }
        let mut answer = match request.operation {
            Operation::Open {
                package_base64,
                package_sha256,
            } => {
                if held.len() >= MAX_SESSIONS {
                    return Err(Failure::unavailable(
                        "core_profile_capacity",
                        "Core already retains eight Profile sessions",
                    )
                    .remedy("Close an existing Profile session before opening another"));
                }
                let bytes = decode_package(&package_base64, &package_sha256)?;
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
                    open_original(scope.clone(), scratch, &bytes, &id, authorized)?;
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
            Operation::GisProjection {
                session_id,
                model_id,
                expected_history,
                label_fields,
            } => {
                validate_label_fields(&label_fields)?;
                let value = owned(&mut held, &scope, &session_id, &model_id, &expected_history)?;
                let checkpoint = value
                    .session
                    .request_at(
                        NativeProfileOperationV1::Checkpoint,
                        Some(expected_history.clone()),
                        DEADLINE,
                        &|| false,
                    )
                    .map_err(native)?;
                authorized()?;
                let projected = project_checkpoint(
                    &checkpoint.bytes,
                    &model_id,
                    &expected_history,
                    &value.coordinate_system,
                    &label_fields,
                )?;
                authorized()?;
                value.touched = Instant::now();
                receipt(
                    &session_id,
                    value,
                    &serde_json::to_vec(&projected).map_err(invalid)?,
                    false,
                )
            }
            Operation::View {
                session_id,
                model_id,
                expected_history,
                options,
                patch,
                style_document,
                cached,
            } => {
                let value = owned(&mut held, &scope, &session_id, &model_id, &expected_history)?;
                let options = serde_json::from_value(options)
                    .map_err(|_| invalid("Use existing native ProfileAtlasOptions"))?;
                let patch = serde_json::from_value(patch)
                    .map_err(|_| invalid("Use existing native DisplayPatch"))?;
                let presentation = serde_json::from_value(value.presentation.clone())
                    .map_err(|_| invalid("Admitted package presentation changed"))?;
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
                let prepared = ds_profile_runtime::native_projection::prepare(
                    &value.display_state,
                    options,
                    &patch,
                    style_document,
                    &presentation,
                )
                .map_err(invalid)?;
                let operation = NativeProfileOperationV1::RuntimeScene {
                    options: serde_json::to_value(&prepared.options).map_err(invalid)?,
                    label_policy: serde_json::to_value(&prepared.labels).map_err(invalid)?,
                    grid_ink: serde_json::from_value(prepared.grid_ink.clone()).map_err(invalid)?,
                    cached,
                };
                let result = value
                    .session
                    .request_at(operation, Some(expected_history.clone()), DEADLINE, &|| {
                        false
                    })
                    .map_err(native)?;
                authorized()?;
                let scene: ds_network_contracts::profile::NativeProfileRuntimeSceneV1 =
                    serde_json::from_slice(&result.bytes).map_err(invalid)?;
                let snapshot = value
                    .session
                    .request_at(
                        NativeProfileOperationV1::State,
                        Some(expected_history.clone()),
                        DEADLINE,
                        &|| false,
                    )
                    .map_err(native)?;
                let state: Value = serde_json::from_slice(&snapshot.bytes).map_err(invalid)?;
                let has_structures = state["table_counts"]["structures"]
                    .as_u64()
                    .is_some_and(|n| n > 0);
                let (display, projection) = ds_profile_runtime::native_projection::project(
                    prepared,
                    &model_id,
                    &expected_history,
                    value.retained_asset_count,
                    has_structures,
                    &scene.scene_json,
                )
                .map_err(invalid)?;
                authorized()?;
                adopt_style_capture(
                    &mut value.style,
                    style_capture,
                    authorized,
                    &native_identity,
                )?;
                value.display_state = display;
                value.touched = Instant::now();
                receipt(
                    &session_id,
                    value,
                    &serde_json::to_vec(&projection).map_err(invalid)?,
                    false,
                )
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
                    | Operation::GisProjection { .. } => unreachable!(),
                };
                let session = owned(&mut held, &scope, &id, &model, &expected)?;
                if matches!(&native_op, NativeProfileOperationV1::Close) {
                    let current = Store::open(database)
                        .map_err(source_error)?
                        .profile_source(&source_scope(&scope), &id)
                        .map_err(source_error)?;
                    if current.as_ref().map(|(header, _)| header) != Some(&session.source) {
                        return Err(Failure::conflict(
                            "core_profile_source_changed",
                            "Exact original changed before native Close",
                        ));
                    }
                    authorized()?;
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
                    let source = session.source.clone();
                    Store::open(database)
                        .map_err(source_error)?
                        .remove_profile_source(&source_scope(&scope), &source)
                        .map_err(source_error)?;
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
/// Read-only projection of the exact worker-owned head. The existing seed owner
/// verifies all assets and replays its authored journal/history; no package,
/// calculated result or cached coordinates replace the retained head.
fn project_checkpoint(
    bytes: &[u8],
    model: &str,
    history: &Value,
    crs: &str,
    fields: &[String],
) -> Result<Value, Failure> {
    validate_label_fields(fields)?;
    if bytes.len() > 64 * 1024 * 1024 {
        return Err(invalid("Native checkpoint exceeds its bound"));
    }
    let checkpoint: NativeProfileOpenV1 = serde_json::from_slice(bytes).map_err(invalid)?;
    if checkpoint.schema != PROFILE_OPEN_SCHEMA {
        return Err(invalid("Native checkpoint schema changed"));
    }
    let seed: ProfileSessionSeed = serde_json::from_str(&checkpoint.seed_json).map_err(invalid)?;
    if seed.model_id.as_str() != model
        || serde_json::to_value(&seed.history).map_err(invalid)? != *history
    {
        return Err(invalid(
            "Native checkpoint differs from retained model/history",
        ));
    }
    let initialized = seed.open().map_err(invalid)?;
    let mut labels = initialized
        .presentation
        .effective_profile_structure_labels();
    labels.fields = serde_json::from_value(json!(fields)).map_err(invalid)?;
    let projection =
        ds_grid_exchange::gis::application_projection(&initialized.session, crs, &labels);
    if projection["revision"] != history["model_revision"] || projection["crs"] != crs {
        return Err(invalid(
            "GIS projection differs from retained revision/declared CRS",
        ));
    }
    Ok(projection)
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
        payload["model_revision"] = json!(held.source_revision);
        payload["coordinate_system"] = json!(held.coordinate_system);
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
    fn physical_style(document: Value) -> Result<(), String> {
        ds_profile_runtime::native_projection::prepare(
            &Default::default(),
            Default::default(),
            &Default::default(),
            document,
            &Default::default(),
        )
        .map(|_| ())
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
        assert!(package_open(bytes).is_err());
    }
    #[test]
    fn gis_checkpoint_requires_exact_model_history_and_declared_crs() {
        let package = ds_grid_exchange::blank_model::create_blank_model(
            &ds_grid_exchange::blank_model::BlankModelRequest {
                model_id: Some("projection-fixture"),
                coordinate_system: Some("EPSG:32735"),
                ..Default::default()
            },
        )
        .unwrap();
        let (checkpoint, _, crs) = package_open(&package.bytes).unwrap();
        let seed: ProfileSessionSeed = serde_json::from_str(&checkpoint.seed_json).unwrap();
        let history = serde_json::to_value(&seed.history).unwrap();
        let bytes = serde_json::to_vec(&checkpoint).unwrap();
        let projected = project_checkpoint(
            &bytes,
            "projection-fixture",
            &history,
            &crs,
            &["number".into()],
        )
        .unwrap();
        assert_eq!(projected["crs"], "EPSG:32735");
        assert_eq!(projected["revision"], history["model_revision"]);
        assert!(project_checkpoint(&bytes, "foreign", &history, &crs, &[]).is_err());
        assert!(project_checkpoint(&bytes, "projection-fixture", &json!({}), &crs, &[]).is_err());
        assert!(
            project_checkpoint(
                &bytes,
                "projection-fixture",
                &history,
                &crs,
                &["arbitrary".into()]
            )
            .is_err()
        );
        let mut crossed: Value = serde_json::from_slice(&bytes).unwrap();
        crossed["schema"] = json!("wrong");
        assert!(
            project_checkpoint(
                &serde_json::to_vec(&crossed).unwrap(),
                "projection-fixture",
                &history,
                &crs,
                &[]
            )
            .is_err()
        );
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
}
