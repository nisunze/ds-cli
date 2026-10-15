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
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    path::Path,
    sync::Mutex,
    time::{Duration, Instant},
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
struct Held {
    scope: Scope,
    package_sha256: String,
    source_revision: u64,
    coordinate_system: String,
    presentation: Value,
    retained_asset_count: usize,
    display_state: ds_command_kernel::profile_display::DisplayState,
    touched: Instant,
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
        held.retain(|_, value| value.touched.elapsed() < IDLE && !value.session.is_closed());
        match request.operation {
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
                let (open, source_revision, coordinate_system) = package_open(&bytes)?;
                let input = serde_json::to_vec(&open)
                    .map_err(|_| invalid("Cannot encode native Profile opening"))?;
                let workers = std::thread::available_parallelism()
                    .map_or(1, usize::from)
                    .min(2);
                let client =
                    CapturedNetworkClient::capture_for_lane(&scope.lane, workers, &|| false)
                        .map_err(native)?;
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
                let value = Held {
                    scope,
                    package_sha256,
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
                    touched: Instant::now(),
                    session,
                };
                let receipt = receipt(&id, &value, &result.bytes, false)?;
                if held.insert(id, value).is_some() {
                    return Err(Failure::internal(
                        "core_profile_session_unavailable",
                        "Private Profile handle collision",
                    ));
                }
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
                let result = session
                    .session
                    .request_at(native_op, Some(expected), DEADLINE, &|| false)
                    .map_err(native)?;
                authorized()?;
                session.touched = Instant::now();
                let closed = result.reply.closed;
                let receipt = receipt(&id, session, &result.bytes, closed)?;
                if closed {
                    held.remove(&id);
                }
                Ok(receipt)
            }
        }
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
            .execute(scope, Path::new("/absent"), request, &|| Ok(()))
            .unwrap_err();
        assert_eq!(refused.code(), "core_profile_session_unavailable");
    }
}
