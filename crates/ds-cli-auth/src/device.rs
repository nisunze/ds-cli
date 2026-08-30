//! Native DS device authorization and inventory host.

use std::fs::File;
use std::io::Read;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use ds_cli_contract::spec::{
    Arg, Authority, Chapter, Command, Effect, Example, Execution, Refusal,
};
use ds_cli_contract::{Context, Failure, Inputs};
use ds_client_core::{
    ClientError, ClientProfile, DeviceAccessSession, DeviceAuthContext, DeviceAuthorizationStatus,
    DeviceBeginPublic, DeviceBeginRequest, DeviceBinding, DeviceCredential, DeviceError,
    DevicePendingAuthorization, DevicePrivateKey, DeviceProtectedCall, DeviceProtectedOperation,
    DeviceSummary, DeviceTransport, ProjectDirectory, ProjectFormSettingsEditor,
    ProjectFormsSnapshot, SecretRequestBody, SolarSnapshot, StoreError, SurveyEntriesChanges,
    SurveyEntriesChangesRequest, SurveyEntriesSelectRequest, SurveyEntriesSelection,
    SurveyEntryCreateReceipt, SurveyEntryCreateRequest, SurveyQueryRequest, SurveyQueryResult,
    TransformerContext, TransportError, TransportResponse, device_secret_json, parse_device_begin,
    parse_device_list, parse_device_read, parse_device_refresh, parse_device_revoke,
    parse_device_status,
};
use serde_json::{Value, json};
use zeroize::{Zeroize, Zeroizing};

use crate::profile::{self, Lane};
use crate::state::{NativeDeviceStore, ProjectContextLease};
use crate::transport::NativeTransport;

const TIMEOUT: Duration = Duration::from_secs(30);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

const LANE: Arg = Arg::value("lane", "<stable|canary>", "Deployment lane.")
    .default("stable")
    .choices(&["stable", "canary"]);
const DEVICE_NAME: Arg = Arg::value(
    "device-name",
    "<name>",
    "Operator-recognizable device name.",
)
.required();
const REQUEST: Arg = Arg::value("request", "<request-id>", "Exact link request id.").required();
const DEVICE_ID: Arg = Arg::value("device", "<device-id>", "Exact public DS device id.").required();

const REFUSALS: &[Refusal] = &[
    Refusal {
        code: "device_state_unavailable",
        when: "protected device state is absent, unsafe, or held by another process",
        remedy: "repair the owner-only DS config directory or retry after the other operation",
    },
    Refusal {
        code: "device_auth_transient",
        when: "the fixed DS device endpoint is temporarily unreachable",
        remedy: "retry without deleting protected device state",
    },
    Refusal {
        code: "device_auth_response_invalid",
        when: "the fixed endpoint returns a response outside the closed device contract",
        remedy: "update ds and retry; do not copy credentials into arguments",
    },
];

pub const BEGIN_COMMAND: Command = Command {
    id: "auth.link.begin",
    path: &["auth", "link", "begin"],
    contract: 1,
    chapter: Chapter::Project,
    summary: "Begin a protected DS device link.",
    purpose: "Creates a client-only Ed25519 key and PKCE secret, sends only public proof, and stores the pending secret bundle in protected native state.",
    effect: Effect::LocalAuthState,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[DEVICE_NAME, LANE],
    output: "Request id, user code, verification URI, expiry, fingerprint, scopes and binding only; never device code, verifier, nonce, key, signature or token.",
    examples: &[Example {
        command: "ds auth link begin --device-name ds-server",
        note: "Approve the shown request from an exact signed-in Desktop.",
        runnable: false,
    }],
    refusals: REFUSALS,
    reference: Some("docs/contracts/unified-identity.md"),
    availability: profile::availability,
};

pub const STATUS_COMMAND: Command = Command {
    id: "auth.link.status",
    path: &["auth", "link", "status"],
    contract: 1,
    chapter: Chapter::Project,
    summary: "Read one pending device-link status.",
    purpose: "Loads the device code only inside protected state and polls the fixed public status endpoint.",
    effect: Effect::ReadOnly,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[REQUEST, LANE],
    output: "Request id, bounded status, expiry and polling interval; no pending secrets.",
    examples: &[Example {
        command: "ds auth link status --request <id>",
        note: "Poll no faster than the returned interval.",
        runnable: false,
    }],
    refusals: REFUSALS,
    reference: Some("docs/contracts/unified-identity.md"),
    availability: profile::availability,
};

pub const COMPLETE_COMMAND: Command = Command {
    id: "auth.link.complete",
    path: &["auth", "link", "complete"],
    contract: 1,
    chapter: Chapter::Project,
    summary: "Complete an approved protected device link.",
    purpose: "Signs the exact completion transcript inside native state, atomically replaces the pending bundle with the durable device credential, and keeps the short-lived access JWT in memory only.",
    effect: Effect::LocalAuthState,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[REQUEST, LANE],
    output: "Public device id, principal attribution and credential expiry only; never token, private key, code, verifier, nonce or signature.",
    examples: &[Example {
        command: "ds auth link complete --request <id>",
        note: "Run after explicit approval.",
        runnable: false,
    }],
    refusals: REFUSALS,
    reference: Some("docs/contracts/unified-identity.md"),
    availability: profile::availability,
};

pub const LIST_COMMAND: Command = Command {
    id: "auth.device.list",
    path: &["auth", "device", "list"],
    contract: 1,
    chapter: Chapter::Project,
    summary: "List devices owned by the linked principal.",
    purpose: "Refreshes a five-minute memory-only DS access session with Ed25519 proof, then reads bounded device inventory.",
    effect: Effect::ReadOnly,
    authority: Authority::HeadlessUser,
    execution: Execution::Sync,
    args: &[LANE],
    output: "Public device inventory and principal attribution; no access token or device private key.",
    examples: &[Example {
        command: "ds auth device list",
        note: "Uses the protected stable-lane device by default.",
        runnable: false,
    }],
    refusals: REFUSALS,
    reference: Some("docs/contracts/unified-identity.md"),
    availability: profile::availability,
};

pub const READ_COMMAND: Command = Command {
    id: "auth.device.read",
    path: &["auth", "device", "read"],
    contract: 1,
    chapter: Chapter::Project,
    summary: "Read one owned device.",
    purpose: "Reads one exact public device record through a memory-only DS access session.",
    effect: Effect::ReadOnly,
    authority: Authority::HeadlessUser,
    execution: Execution::Sync,
    args: &[DEVICE_ID, LANE],
    output: "One public device record; other-owner ids are indistinguishable from not found.",
    examples: &[Example {
        command: "ds auth device read --device <id>",
        note: "Use an exact id from device list.",
        runnable: false,
    }],
    refusals: REFUSALS,
    reference: Some("docs/contracts/unified-identity.md"),
    availability: profile::availability,
};

pub const REVOKE_COMMAND: Command = Command {
    id: "auth.device.revoke",
    path: &["auth", "device", "revoke"],
    contract: 1,
    chapter: Chapter::Project,
    summary: "Revoke one owned device.",
    purpose: "Deletes one server-side device authority; revoking the current device also atomically removes its protected local credential.",
    effect: Effect::GlobalWrite,
    authority: Authority::HeadlessUser,
    execution: Execution::Sync,
    args: &[DEVICE_ID, LANE],
    output: "Revoked device id, status and timestamp only.",
    examples: &[Example {
        command: "ds auth device revoke --device <id> --yes",
        note: "Requires explicit confirmation.",
        runnable: false,
    }],
    refusals: REFUSALS,
    reference: Some("docs/contracts/unified-identity.md"),
    availability: profile::availability,
};

pub fn run_begin(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
    let lane = lane(inputs)?;
    let (profile, catalog_digest, gateway_key) = profile::load_device(lane)?;
    let state_key = state_key(&profile);
    let mut store = store_reserve(&state_key)?;
    let binding = DeviceBinding::for_profile(&profile, &catalog_digest).map_err(device_failure)?;
    let mut key_bytes = random_bytes::<32>()?;
    let key = DevicePrivateKey::from_secret_bytes(key_bytes);
    key_bytes.zeroize();
    let verifier = random_token::<32>()?;
    let nonce = random_token::<24>()?;
    let request = DeviceBeginRequest::new(
        inputs.require("device-name")?,
        platform(),
        &key.public_key_base64url(),
        &nonce,
        &verifier,
        &binding,
    )
    .map_err(device_failure)?;
    let mut transport = NativeDeviceTransport::new(&profile, gateway_key);
    let response = transport
        .begin(device_secret_json(&request).map_err(device_failure)?)
        .map_err(transport_failure)?;
    let result = parse_device_begin(response, &binding).map_err(device_failure)?;
    let pending =
        DevicePendingAuthorization::new(result, verifier, nonce, key).map_err(device_failure)?;
    let public = pending.public();
    let encoded = pending.encode_protected().map_err(device_failure)?;
    store
        .compare_and_swap(&state_key, None, Some(encoded.as_bytes()))
        .map_err(store_failure)?;
    release(&mut store, &state_key)?;
    Ok(begin_public_json(public))
}

fn begin_public_json(public: DeviceBeginPublic) -> Value {
    json!({
        "request_id": public.request_id,
        "user_code": public.user_code,
        "verification_uri": public.verification_uri,
        "expires_at": public.expires_at,
        "poll_interval_seconds": public.poll_interval_seconds,
        "device_fingerprint": public.device_fingerprint,
        "scopes": public.scopes,
        "binding": public.binding,
    })
}

pub fn run_status(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
    let lane = lane(inputs)?;
    let (profile, _, gateway_key) = profile::load_device(lane)?;
    let (mut store, bytes) = store_load(&state_key(&profile))?;
    let pending =
        DevicePendingAuthorization::decode_protected(&bytes, &profile).map_err(device_failure)?;
    let public = pending.public();
    exact_request(inputs, &public.request_id)?;
    let mut transport = NativeDeviceTransport::new(&profile, gateway_key);
    let response = transport
        .status(device_secret_json(&pending.status_request()).map_err(device_failure)?)
        .map_err(transport_failure)?;
    let result = parse_device_status(response).map_err(device_failure)?;
    release(&mut store, &state_key(&profile))?;
    Ok(json!({
        "request_id": public.request_id,
        "status": status_token(result.status),
        "expires_at": result.expires_at,
        "poll_interval_seconds": result.poll_interval_seconds,
    }))
}

pub fn run_complete(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
    let lane = lane(inputs)?;
    let (profile, _, gateway_key) = profile::load_device(lane)?;
    let key = state_key(&profile);
    let (mut store, bytes) = store_load(&key)?;
    let pending =
        DevicePendingAuthorization::decode_protected(&bytes, &profile).map_err(device_failure)?;
    exact_request(inputs, &pending.public().request_id)?;
    let mut transport = NativeDeviceTransport::new(&profile, gateway_key);
    let response = transport
        .complete(
            device_secret_json(&pending.complete_request().map_err(device_failure)?)
                .map_err(device_failure)?,
        )
        .map_err(transport_failure)?;
    let (credential, access) = pending
        .parse_complete(response, unix_seconds())
        .map_err(device_failure)?;
    let replacement = credential.encode_protected().map_err(device_failure)?;
    store
        .compare_and_swap(&key, Some(&bytes), Some(replacement.as_bytes()))
        .map_err(store_failure)?;
    release(&mut store, &key)?;
    drop(access);
    Ok(json!({
        "linked": true,
        "device_id": credential.device_id(),
        "principal": { "uid": credential.uid(), "email": credential.email() },
        "credential_expires_at": credential.credential_expires_at(),
        "lane": lane.token(),
        "credential_audience_sha256": profile.credential_audience_sha256(),
    }))
}

pub fn run_list(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
    protected(inputs, DeviceProtectedOperation::List, |response| {
        let result = parse_device_list(response).map_err(device_failure)?;
        Ok(json!({ "devices": result.devices.iter().map(summary_json).collect::<Vec<_>>() }))
    })
}

pub fn run_read(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
    let id = inputs.require("device")?.to_owned();
    protected(inputs, DeviceProtectedOperation::Read(&id), |response| {
        let result = parse_device_read(response).map_err(device_failure)?;
        Ok(json!({ "device": summary_json(&result.device) }))
    })
}

pub fn run_revoke(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
    let id = inputs.require("device")?.to_owned();
    protected(inputs, DeviceProtectedOperation::Revoke(&id), |response| {
        let result = parse_device_revoke(response).map_err(device_failure)?;
        Ok(json!({
            "device_id": result.device_id,
            "status": result.status,
            "revoked": result.revoked,
            "revoked_at": result.revoked_at,
        }))
    })
}

fn protected(
    inputs: &Inputs,
    operation: DeviceProtectedOperation<'_>,
    parse: impl FnOnce(TransportResponse) -> Result<Value, Failure>,
) -> Result<Value, Failure> {
    let lane = lane(inputs)?;
    let (profile, catalog_digest, gateway_key) = profile::load_device(lane)?;
    let key = state_key(&profile);
    let (mut store, bytes) = store_load(&key)?;
    let credential =
        DeviceCredential::decode_protected(&bytes, &profile).map_err(device_failure)?;
    let nonce = random_token::<24>()?;
    let timestamp = unix_seconds();
    let binding = DeviceBinding::for_profile(&profile, &catalog_digest).map_err(device_failure)?;
    let refresh = credential
        .refresh_request(timestamp, &nonce, &binding)
        .map_err(device_failure)?;
    let mut transport = NativeDeviceTransport::new(&profile, gateway_key);
    let response = transport
        .refresh(device_secret_json(&refresh).map_err(device_failure)?)
        .map_err(transport_failure)?;
    let (session, _) = parse_device_refresh(response, timestamp).map_err(device_failure)?;
    let response = transport
        .protected(session.protected_call(operation))
        .map_err(transport_failure)?;
    let result = parse(response)?;
    let revoke_current = result.get("revoked").and_then(Value::as_bool) == Some(true)
        && result.get("device_id").and_then(Value::as_str) == Some(credential.device_id());
    if revoke_current {
        store
            .compare_and_swap(&key, Some(&bytes), None)
            .map_err(store_failure)?;
    }
    release(&mut store, &key)?;
    Ok(result)
}

/// Non-network protected-provider observation for registry arbitration.
pub fn probe_identity(
    lane: Lane,
) -> Result<Option<(crate::ProviderIdentity, Option<String>)>, Failure> {
    let (profile, _, _) = profile::load_device(lane)?;
    let key = state_key(&profile);
    let mut store = NativeDeviceStore::open()?;
    store.acquire(&key).map_err(store_failure)?;
    let credential = match store.load(&key).map_err(store_failure)? {
        Some(bytes) => {
            let bytes = Zeroizing::new(bytes);
            match DeviceCredential::decode_protected(&bytes, &profile) {
                Ok(credential) => Some(credential),
                Err(_)
                    if DevicePendingAuthorization::decode_protected(&bytes, &profile).is_ok() =>
                {
                    None
                }
                Err(error) => {
                    let _ = store.release(&key);
                    return Err(device_failure(error));
                }
            }
        }
        None => None,
    };
    release(&mut store, &key)?;
    let Some(credential) = credential else {
        return Ok(None);
    };
    let identity = crate::ProviderIdentity::new(
        lane.token(),
        profile.credential_audience_sha256(),
        credential.uid(),
    )?;
    let selected = ProjectContextLease::acquire(&profile)?
        .load_snapshot(&profile, credential.uid(), credential.email())?
        .map(|project| project.project_id().to_owned());
    Ok(Some((identity, selected)))
}

macro_rules! fixed_device_call {
    ($self:ident, $method:ident $(, $argument:expr)*) => {{
        let now = unix_seconds();
        let authorization = $self
            .credential
            .authorize_api(
                &$self.access,
                &$self.profile,
                $self.credential.uid(),
                now,
            )
            .map_err(|_| ClientError::from(TransportError::Unreachable))?;
        authorization.$method(&mut $self.transport, $($argument,)* now)
    }};
}

/// One refreshed memory-only device session for the existing closed project,
/// Survey, and Solar calls.
pub struct DeviceSession {
    profile: ClientProfile,
    credential: DeviceCredential,
    access: DeviceAccessSession,
    transport: NativeTransport,
}

impl DeviceSession {
    pub fn profile(&self) -> &ClientProfile {
        &self.profile
    }
    pub fn context(&self) -> DeviceAuthContext {
        self.credential.auth_context()
    }
    pub fn list_projects(&mut self) -> Result<ProjectDirectory, ClientError> {
        fixed_device_call!(self, list_projects)
    }
    pub fn transformer_context(
        &mut self,
        project: &str,
        transformer: &str,
    ) -> Result<TransformerContext, ClientError> {
        fixed_device_call!(self, transformer_context, project, transformer)
    }
    pub fn project_forms(&mut self, project: &str) -> Result<ProjectFormsSnapshot, ClientError> {
        fixed_device_call!(self, project_forms, project)
    }
    pub fn project_form_editor(
        &mut self,
        project: &str,
        form_slug: &str,
    ) -> Result<ProjectFormSettingsEditor, ClientError> {
        fixed_device_call!(self, project_form_editor, project, form_slug)
    }
    pub fn solar_snapshot(
        &mut self,
        project: &str,
        template: &str,
    ) -> Result<SolarSnapshot, ClientError> {
        fixed_device_call!(self, solar_snapshot, project, template)
    }
    pub fn survey_query(
        &mut self,
        project: &str,
        request: &SurveyQueryRequest,
    ) -> Result<SurveyQueryResult, ClientError> {
        fixed_device_call!(self, survey_query, project, request)
    }
    pub fn survey_entries_select(
        &mut self,
        project: &str,
        request: &SurveyEntriesSelectRequest,
    ) -> Result<SurveyEntriesSelection, ClientError> {
        fixed_device_call!(self, survey_entries_select, project, request)
    }
    pub fn survey_entries_changes(
        &mut self,
        project: &str,
        request: &SurveyEntriesChangesRequest,
    ) -> Result<SurveyEntriesChanges, ClientError> {
        fixed_device_call!(self, survey_entries_changes, project, request)
    }
    pub fn survey_entry_create(
        &mut self,
        project: &str,
        request: &SurveyEntryCreateRequest,
    ) -> Result<SurveyEntryCreateReceipt, ClientError> {
        fixed_device_call!(self, survey_entry_create, project, request)
    }
}

/// Decode durable device authority and refresh one short-lived access JWT.
pub fn restore_session(lane: Lane) -> Result<Option<DeviceSession>, Failure> {
    let (profile, catalog_digest, gateway_key) = profile::load_device(lane)?;
    let credential = load_credential(&profile)?;
    let Some(credential) = credential else {
        return Ok(None);
    };
    let timestamp = unix_seconds();
    let nonce = random_token::<24>()?;
    let binding = DeviceBinding::for_profile(&profile, &catalog_digest).map_err(device_failure)?;
    let refresh = credential
        .refresh_request(timestamp, &nonce, &binding)
        .map_err(device_failure)?;
    let mut device_transport = NativeDeviceTransport::new(&profile, gateway_key);
    let response = device_transport
        .refresh(device_secret_json(&refresh).map_err(device_failure)?)
        .map_err(transport_failure)?;
    let (access, _) = parse_device_refresh(response, timestamp).map_err(device_failure)?;
    Ok(Some(DeviceSession {
        profile,
        credential,
        access,
        transport: NativeTransport,
    }))
}

pub fn probe_context(lane: Lane) -> Result<Option<DeviceAuthContext>, Failure> {
    let profile = profile::load(lane)?;
    Ok(load_credential(&profile)?.map(|credential| credential.auth_context()))
}

fn load_credential(profile: &ClientProfile) -> Result<Option<DeviceCredential>, Failure> {
    let key = state_key(profile);
    let mut store = NativeDeviceStore::open()?;
    store.acquire(&key).map_err(store_failure)?;
    let credential = match store.load(&key).map_err(store_failure)? {
        None => None,
        Some(bytes) => {
            let bytes = Zeroizing::new(bytes);
            match DeviceCredential::decode_protected(&bytes, profile) {
                Ok(credential) => Some(credential),
                Err(_) if DevicePendingAuthorization::decode_protected(&bytes, profile).is_ok() => {
                    None
                }
                Err(error) => {
                    let _ = store.release(&key);
                    return Err(device_failure(error));
                }
            }
        }
    };
    release(&mut store, &key)?;
    Ok(credential)
}

fn summary_json(value: &DeviceSummary) -> Value {
    json!({
        "device_id": value.device_id, "name": value.name, "platform": value.platform,
        "fingerprint": value.fingerprint, "principal": { "uid": value.principal.uid, "email": value.principal.email },
        "approved_by": { "uid": value.approved_by.uid, "email": value.approved_by.email },
        "status": value.status, "scopes": value.scopes, "binding": value.binding,
        "approved_at": value.approved_at, "completed_at": value.completed_at,
        "created_at": value.created_at, "last_used_at": value.last_used_at,
        "last_refreshed_at": value.last_refreshed_at, "credential_expires_at": value.credential_expires_at,
        "revoked_at": value.revoked_at,
    })
}

fn lane(inputs: &Inputs) -> Result<Lane, Failure> {
    Lane::parse(inputs.value("lane").unwrap_or("stable"))
}

pub fn lane_from_token(value: &str) -> Result<Lane, Failure> {
    Lane::parse(value)
}

fn exact_request(inputs: &Inputs, stored: &str) -> Result<(), Failure> {
    if inputs.require("request")? == stored {
        Ok(())
    } else {
        Err(Failure::conflict(
            "device_request_mismatch",
            "the requested link id is not the protected pending link for this lane",
        ))
    }
}

fn state_key(profile: &ds_client_core::ClientProfile) -> String {
    format!(
        "device:{}:{}",
        profile.lane().token(),
        profile.credential_audience_sha256()
    )
}

fn store_reserve(key: &str) -> Result<NativeDeviceStore, Failure> {
    let mut store = NativeDeviceStore::open()?;
    store.acquire(key).map_err(store_failure)?;
    let current = store.load(key).map_err(store_failure)?.map(Zeroizing::new);
    if current.is_some() {
        let _ = store.release(key);
        return Err(Failure::conflict(
            "device_state_exists",
            "this lane already has a pending link or durable device credential",
        )
        .remedy("complete or revoke the existing device before beginning another link"));
    }
    Ok(store)
}

fn store_load(key: &str) -> Result<(NativeDeviceStore, Zeroizing<Vec<u8>>), Failure> {
    let mut store = NativeDeviceStore::open()?;
    store.acquire(key).map_err(store_failure)?;
    let Some(bytes) = store.load(key).map_err(store_failure)? else {
        let _ = store.release(key);
        return Err(Failure::unauthorized(
            "device_not_linked",
            "no protected DS device state exists for this lane",
        )
        .remedy("run ds auth link begin"));
    };
    Ok((store, Zeroizing::new(bytes)))
}

fn release(store: &mut NativeDeviceStore, key: &str) -> Result<(), Failure> {
    store.release(key).map_err(store_failure)
}

fn status_token(status: DeviceAuthorizationStatus) -> &'static str {
    match status {
        DeviceAuthorizationStatus::Pending => "pending",
        DeviceAuthorizationStatus::Approved => "approved",
        DeviceAuthorizationStatus::Denied => "denied",
        DeviceAuthorizationStatus::Expired => "expired",
        DeviceAuthorizationStatus::Consumed => "consumed",
    }
}

fn random_token<const N: usize>() -> Result<String, Failure> {
    let mut bytes = random_bytes::<N>()?;
    let token = URL_SAFE_NO_PAD.encode(bytes);
    bytes.zeroize();
    Ok(token)
}

fn random_bytes<const N: usize>() -> Result<[u8; N], Failure> {
    let mut bytes = [0_u8; N];
    #[cfg(unix)]
    {
        File::open("/dev/urandom")
            .and_then(|mut file| file.read_exact(&mut bytes))
            .map_err(|_| {
                Failure::unavailable(
                    "device_rng_unavailable",
                    "the operating system cryptographic RNG is unavailable",
                )
            })?;
        Ok(bytes)
    }
    #[cfg(not(unix))]
    {
        Err(Failure::unavailable(
            "device_rng_unavailable",
            "this build has no protected operating-system RNG adapter",
        ))
    }
}

fn unix_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

const fn platform() -> &'static str {
    if cfg!(target_os = "windows") {
        "windows"
    } else if cfg!(target_os = "macos") {
        "macos"
    } else {
        "linux"
    }
}

fn device_failure(_: DeviceError) -> Failure {
    Failure::failed(
        "device_auth_response_invalid",
        "the device authorization data did not satisfy the closed contract",
    )
    .remedy("retry once, then update ds; never copy credential material into arguments")
}

fn transport_failure(_: TransportError) -> Failure {
    Failure::unavailable(
        "device_auth_transient",
        "the fixed DS device authorization endpoint is temporarily unreachable",
    )
    .remedy("retry without changing protected device state")
}

fn store_failure(error: StoreError) -> Failure {
    match error {
        StoreError::Conflict => Failure::conflict(
            "device_state_conflict",
            "another process holds or changed protected device state",
        )
        .remedy("retry after the other device operation finishes"),
        StoreError::Unavailable | StoreError::UnsafeOrUnreadable => Failure::unavailable(
            "device_state_unavailable",
            "protected device state is unavailable or unsafe",
        )
        .remedy("repair the owner-only DS config directory and retry"),
    }
}

struct NativeDeviceTransport {
    origin: String,
    gateway_key: String,
}

impl NativeDeviceTransport {
    fn new(profile: &ds_client_core::ClientProfile, gateway_key: String) -> Self {
        Self {
            origin: profile.gateway_origin().trim_end_matches('/').to_owned(),
            gateway_key,
        }
    }

    fn post(
        &self,
        path: &str,
        body: SecretRequestBody,
    ) -> Result<TransportResponse, TransportError> {
        let response = ureq::post(format!("{}{}", self.origin, path))
            .header("Accept", "application/json")
            .header("Content-Type", "application/json")
            .header("X-App-Id", ds_client_core::NATIVE_CLIENT_ID)
            .header("x-api-key", &self.gateway_key)
            .config()
            .max_redirects(0)
            .http_status_as_error(false)
            .timeout_connect(Some(CONNECT_TIMEOUT))
            .timeout_global(Some(TIMEOUT))
            .build()
            .send(body.as_bytes())
            .map_err(classify)?;
        bounded(response)
    }
}

impl Drop for NativeDeviceTransport {
    fn drop(&mut self) {
        self.gateway_key.zeroize();
    }
}

impl DeviceTransport for NativeDeviceTransport {
    fn begin(&mut self, body: SecretRequestBody) -> Result<TransportResponse, TransportError> {
        self.post("/api/v1/auth/device/begin", body)
    }
    fn status(&mut self, body: SecretRequestBody) -> Result<TransportResponse, TransportError> {
        self.post("/api/v1/auth/device/status", body)
    }
    fn complete(&mut self, body: SecretRequestBody) -> Result<TransportResponse, TransportError> {
        self.post("/api/v1/auth/device/complete", body)
    }
    fn refresh(&mut self, body: SecretRequestBody) -> Result<TransportResponse, TransportError> {
        self.post("/api/v1/auth/device/refresh", body)
    }
    fn protected(
        &mut self,
        call: DeviceProtectedCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        let path = call.path().map_err(|_| TransportError::Unreachable)?;
        let mut bearer = format!("Bearer {}", call.bearer_token());
        let request = match call.method() {
            "GET" => ureq::get(format!("{}{}", self.origin, path)),
            "DELETE" => ureq::delete(format!("{}{}", self.origin, path)),
            _ => return Err(TransportError::Unreachable),
        };
        let result = request
            .header("Accept", "application/json")
            .header("X-App-Id", ds_client_core::NATIVE_CLIENT_ID)
            .header("x-api-key", &self.gateway_key)
            .header("Authorization", &bearer)
            .config()
            .max_redirects(0)
            .http_status_as_error(false)
            .timeout_connect(Some(CONNECT_TIMEOUT))
            .timeout_global(Some(TIMEOUT))
            .build()
            .call();
        bearer.zeroize();
        bounded(result.map_err(classify)?)
    }
}

fn bounded(
    mut response: ureq::http::Response<ureq::Body>,
) -> Result<TransportResponse, TransportError> {
    let status = response.status().as_u16();
    let mut body = Vec::new();
    response
        .body_mut()
        .with_config()
        .limit((ds_client_core::DEVICE_ACCESS_RESPONSE_LIMIT + 1) as u64)
        .reader()
        .read_to_end(&mut body)
        .map_err(|_| TransportError::Unreachable)?;
    Ok(TransportResponse::new(status, body))
}

fn classify(error: ureq::Error) -> TransportError {
    if matches!(error, ureq::Error::Timeout(_)) {
        TransportError::TimedOut
    } else {
        TransportError::Unreachable
    }
}

pub fn render(value: &Value) -> String {
    if let Some(id) = value.get("request_id").and_then(Value::as_str) {
        return format!("device authorization {id}");
    }
    if let Some(id) = value.get("device_id").and_then(Value::as_str) {
        return format!("device {id}");
    }
    if let Some(devices) = value.get("devices").and_then(Value::as_array) {
        return format!("{} device(s)", devices.len());
    }
    "device authorization updated".to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn begin_projection_has_only_public_handoff_fields() {
        let value = begin_public_json(DeviceBeginPublic {
            request_id: "request-1".to_owned(),
            user_code: "BLUE-OTTER".to_owned(),
            verification_uri: "https://example.test/device".to_owned(),
            expires_at: "2026-08-30T12:00:00Z".to_owned(),
            poll_interval_seconds: 5,
            device_fingerprint: format!("sha256:{}", "a".repeat(64)),
            scopes: vec!["ds.api".to_owned()],
            binding: DeviceBinding {
                lane: "stable".to_owned(),
                audience: "b".repeat(64),
                profile_digest: format!("sha256:{}", "c".repeat(64)),
                catalog_digest: format!("sha256:{}", "d".repeat(64)),
            },
        });
        let transcript = serde_json::to_string(&value).unwrap();
        for secret in [
            "device_code",
            "code_verifier",
            "private_key",
            "access_token",
            "signature",
            "nonce",
        ] {
            assert!(!transcript.contains(secret), "CLI output exposed {secret}");
        }
    }
}
