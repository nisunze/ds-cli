//! Process-lifetime Profile ownership. Parsing/engineering stay in their existing
//! Grid exchange and managed Network owners; no browser, SDK or caller argv.
use base64::{engine::general_purpose::STANDARD, Engine};
use ds_cli_contract::Failure;
use ds_grid_exchange::profile_session::ProfileSessionSeed;
use ds_network_client::{CapturedNetworkClient, NetworkFailure, ProfileSession};
use ds_network_contracts::profile::NativeProfileGridInkV1;
use ds_network_contracts::profile::{
    NativeProfileOpenV1, NativeProfileOperationV1, PROFILE_OPEN_SCHEMA,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
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
                    Operation::Open { .. } => unreachable!(),
                };
                let session = held
                    .get_mut(&id)
                    .filter(|v| v.scope == scope && v.session.model_id() == model)
                    .ok_or_else(|| {
                        Failure::unavailable(
                            "core_profile_session_unavailable",
                            "No retained Profile belongs to this exact account/lane/project/model",
                        )
                        .remedy("Explicitly reopen the exact package through the same Core owner")
                    })?;
                if session.session.history() != &expected {
                    return Err(Failure::conflict(
                        "core_profile_history_conflict",
                        "Profile history changed before the request",
                    )
                    .remedy(
                        "Read the retained session head and submit the intended operation again",
                    ));
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
                    held.remove(&id);
                }
                Ok(receipt)
            }
        }
    }
}
fn receipt(id: &str, held: &Held, bytes: &[u8], closed: bool) -> Result<Value, Failure> {
    if bytes.len() > 64 * 1024 * 1024 {
        return Err(invalid(
            "Profile artifact exceeds the 64 MiB host response bound",
        ));
    }
    // Exact native JSON strings inside snapshot/checkpoint stay opaque text.
    let payload: Value = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(bytes)
            .map_err(|_| invalid("Native Profile artifact is not its declared JSON"))?
    };
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
