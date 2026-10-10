use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use ed25519_dalek::{Signer, SigningKey};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use zeroize::Zeroize;

use crate::project_report::{
    CompoundedArchiveCatalog, CompoundedReportReceipt, CompoundedReportRequest, RetirementAction,
    RetirementReceipt, RetirementRequest, TransformerInventory, TransformerSet,
    TransformerStatusList,
};
use crate::weak_network::transient_status;
use crate::{
    AuthorizedTransport, ClientError, ClientProfile, ErrorKind, ProjectDirectory,
    ProjectFormSettingsEditor, ProjectFormsSnapshot, SecretRequestBody, SolarSnapshot, StoreError,
    SurveyEntriesChanges, SurveyEntriesChangesRequest, SurveyEntriesSelectRequest,
    SurveyEntriesSelection, SurveyEntryCreateReceipt, SurveyEntryCreateRequest, SurveyQueryRequest,
    SurveyQueryResult, TileOperationResult, TilePreflight, TileType, TransformerContext,
    TransportError, TransportResponse, WeakNetwork,
};

pub const DEVICE_AUTH_SCHEMA: &str = "ds.device_authorization.v1";
pub const DEVICE_SCOPE: &str = "ds.api";
pub const AUTH_CONTEXT_MISMATCH: &str = "AUTH_CONTEXT_MISMATCH";
pub const DEVICE_ACCESS_RESPONSE_LIMIT: usize = 256 * 1024;
pub const DEVICE_ACCESS_TTL_MAX_SECONDS: u64 = 600;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeviceBinding {
    pub lane: String,
    pub audience: String,
    pub profile_digest: String,
    pub catalog_digest: String,
}

impl DeviceBinding {
    pub fn for_profile(profile: &ClientProfile, catalog_sha256: &str) -> Result<Self, DeviceError> {
        let catalog_digest = prefixed_digest(catalog_sha256)?;
        Ok(Self {
            lane: profile.lane().token().to_owned(),
            audience: profile.credential_audience_sha256().to_owned(),
            profile_digest: format!("sha256:{}", profile.profile_sha256()),
            catalog_digest,
        })
    }
}

pub struct DevicePrivateKey(SigningKey);

impl DevicePrivateKey {
    /// Construct from 32 bytes supplied by the host's cryptographic RNG.
    pub fn from_secret_bytes(bytes: [u8; 32]) -> Self {
        Self(SigningKey::from_bytes(&bytes))
    }

    pub fn public_key_base64url(&self) -> String {
        URL_SAFE_NO_PAD.encode(self.0.verifying_key().as_bytes())
    }

    pub fn complete_signature(
        &self,
        request_id: &str,
        device_code: &str,
        code_verifier: &str,
        nonce: &str,
    ) -> Result<String, DeviceError> {
        bounded(request_id, 128)?;
        bounded(device_code, 512)?;
        validate_verifier(code_verifier)?;
        validate_nonce(nonce)?;
        let transcript = format!(
            "DS-DEVICE-COMPLETE-V1\n{}\n{}\n{}\n{}",
            request_id,
            sha256_hex(device_code.as_bytes()),
            sha256_hex(code_verifier.as_bytes()),
            nonce,
        );
        Ok(URL_SAFE_NO_PAD.encode(self.0.sign(transcript.as_bytes()).to_bytes()))
    }

    pub fn refresh_signature(
        &self,
        device_id: &str,
        timestamp: u64,
        nonce: &str,
        binding: &DeviceBinding,
    ) -> Result<String, DeviceError> {
        bounded(device_id, 128)?;
        validate_nonce(nonce)?;
        validate_binding(binding)?;
        let transcript = format!(
            "DS-DEVICE-REFRESH-V1\n{}\n{}\n{}\n{}\n{}\n{}\n{}",
            device_id,
            timestamp,
            nonce,
            binding.lane,
            binding.audience,
            binding.profile_digest,
            binding.catalog_digest,
        );
        Ok(URL_SAFE_NO_PAD.encode(self.0.sign(transcript.as_bytes()).to_bytes()))
    }
}

#[derive(Serialize)]
struct PublicKey {
    alg: &'static str,
    encoding: &'static str,
    value: String,
}

#[derive(Serialize)]
struct Pkce {
    method: &'static str,
    challenge: String,
}

#[derive(Serialize)]
pub struct DeviceBeginRequest {
    schema: &'static str,
    device_name: String,
    platform: String,
    public_key: PublicKey,
    nonce: String,
    pkce: Pkce,
    scopes: [&'static str; 1],
    binding: DeviceBinding,
}

impl DeviceBeginRequest {
    pub fn new(
        device_name: &str,
        platform: &str,
        public_key: &str,
        nonce: &str,
        code_verifier: &str,
        binding: &DeviceBinding,
    ) -> Result<Self, DeviceError> {
        bounded(device_name, 100)?;
        if !matches!(platform, "windows" | "linux" | "macos") {
            return Err(DeviceError::Input);
        }
        validate_public_key(public_key)?;
        validate_nonce(nonce)?;
        validate_verifier(code_verifier)?;
        validate_binding(binding)?;
        let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(code_verifier.as_bytes()));
        Ok(Self {
            schema: DEVICE_AUTH_SCHEMA,
            device_name: device_name.to_owned(),
            platform: platform.to_owned(),
            public_key: PublicKey {
                alg: "Ed25519",
                encoding: "base64url",
                value: public_key.to_owned(),
            },
            nonce: nonce.to_owned(),
            pkce: Pkce {
                method: "S256",
                challenge,
            },
            scopes: [DEVICE_SCOPE],
            binding: binding.clone(),
        })
    }
}

#[derive(Clone, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DeviceStatusRequest<'a> {
    request_id: &'a str,
    device_code: &'a str,
}

#[derive(Clone, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DeviceCompleteRequest<'a> {
    request_id: &'a str,
    device_code: &'a str,
    code_verifier: &'a str,
    nonce: &'a str,
    signature: String,
}

impl<'a> DeviceCompleteRequest<'a> {
    fn signed(
        key: &DevicePrivateKey,
        request_id: &'a str,
        device_code: &'a str,
        code_verifier: &'a str,
        nonce: &'a str,
    ) -> Result<Self, DeviceError> {
        Ok(Self {
            request_id,
            device_code,
            code_verifier,
            nonce,
            signature: key.complete_signature(request_id, device_code, code_verifier, nonce)?,
        })
    }
}

#[derive(Clone, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DeviceRefreshRequest {
    device_id: String,
    timestamp: u64,
    nonce: String,
    signature: String,
    binding: DeviceBinding,
}

impl DeviceRefreshRequest {
    fn signed(
        key: &DevicePrivateKey,
        device_id: &str,
        timestamp: u64,
        nonce: &str,
        binding: &DeviceBinding,
    ) -> Result<Self, DeviceError> {
        Ok(Self {
            device_id: device_id.to_owned(),
            timestamp,
            nonce: nonce.to_owned(),
            signature: key.refresh_signature(device_id, timestamp, nonce, binding)?,
            binding: binding.clone(),
        })
    }
    pub fn binding(&self) -> &DeviceBinding {
        &self.binding
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeviceAuthorizationStatus {
    Pending,
    Approved,
    Denied,
    Expired,
    Consumed,
}

impl DeviceAuthorizationStatus {
    /// The wire token, as `deviceauth` records it.
    pub const fn token(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Approved => "approved",
            Self::Denied => "denied",
            Self::Expired => "expired",
            Self::Consumed => "consumed",
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeviceBeginResult {
    pub request_id: String,
    device_code: SecretString,
    pub user_code: String,
    pub verification_uri: String,
    pub expires_at: String,
    pub poll_interval_seconds: u64,
    pub device_fingerprint: String,
    pub scopes: Vec<String>,
    pub binding: DeviceBinding,
}

impl DeviceBeginResult {
    fn status_request(&self) -> DeviceStatusRequest<'_> {
        DeviceStatusRequest {
            request_id: &self.request_id,
            device_code: &self.device_code.0,
        }
    }
    fn complete_request<'a>(
        &'a self,
        key: &DevicePrivateKey,
        code_verifier: &'a str,
        nonce: &'a str,
    ) -> Result<DeviceCompleteRequest<'a>, DeviceError> {
        DeviceCompleteRequest::signed(
            key,
            &self.request_id,
            &self.device_code.0,
            code_verifier,
            nonce,
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DeviceBeginPublic {
    pub request_id: String,
    pub user_code: String,
    pub verification_uri: String,
    pub expires_at: String,
    pub poll_interval_seconds: u64,
    pub device_fingerprint: String,
    pub scopes: Vec<String>,
    pub binding: DeviceBinding,
}

pub struct DevicePendingAuthorization {
    result: DeviceBeginResult,
    code_verifier: SecretString,
    nonce: SecretString,
    private_key: DevicePrivateKey,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredPendingAuthorization {
    schema: String,
    request_id: String,
    device_code: SecretString,
    user_code: String,
    verification_uri: String,
    expires_at: String,
    poll_interval_seconds: u64,
    device_fingerprint: String,
    scopes: Vec<String>,
    binding: DeviceBinding,
    code_verifier: SecretString,
    nonce: SecretString,
    private_key: SecretString,
}

impl DevicePendingAuthorization {
    pub fn new(
        result: DeviceBeginResult,
        code_verifier: String,
        nonce: String,
        private_key: DevicePrivateKey,
    ) -> Result<Self, DeviceError> {
        validate_verifier(&code_verifier)?;
        validate_nonce(&nonce)?;
        Ok(Self {
            result,
            code_verifier: SecretString(code_verifier),
            nonce: SecretString(nonce),
            private_key,
        })
    }
    pub fn public(&self) -> DeviceBeginPublic {
        DeviceBeginPublic {
            request_id: self.result.request_id.clone(),
            user_code: self.result.user_code.clone(),
            verification_uri: self.result.verification_uri.clone(),
            expires_at: self.result.expires_at.clone(),
            poll_interval_seconds: self.result.poll_interval_seconds,
            device_fingerprint: self.result.device_fingerprint.clone(),
            scopes: self.result.scopes.clone(),
            binding: self.result.binding.clone(),
        }
    }
    /// A pending link may be replaced only after its captured server deadline.
    /// Invalid deadlines do not authorize replacement of protected state.
    pub fn is_expired(&self, now_unix_seconds: u64) -> Result<bool, DeviceError> {
        let deadline = time::OffsetDateTime::parse(
            &self.result.expires_at,
            &time::format_description::well_known::Rfc3339,
        )
        .map_err(|_| DeviceError::Response)?;
        Ok(i128::from(now_unix_seconds) * 1_000_000_000 >= deadline.unix_timestamp_nanos())
    }

    pub fn status_request(&self) -> DeviceStatusRequest<'_> {
        self.result.status_request()
    }
    /// Ask the fixed endpoint where this link stands, retrying a blink under
    /// `weak_network`. The status read changes nothing server-side, so the
    /// same request is simply sent again.
    ///
    /// Begin and complete have no such twin on purpose: begin creates a
    /// request and complete consumes one, so a retry after an answer that was
    /// sent but lost would create a second request or find the first consumed.
    pub fn observe_status<T: DeviceTransport>(
        &self,
        transport: &mut T,
        weak_network: &WeakNetwork,
    ) -> Result<DeviceStatusResult, DeviceCallFailure> {
        let response = over_weak_network(weak_network, || {
            let body = secret_json(&self.status_request()).map_err(DeviceCallFailure::Device)?;
            Ok(transport.status(body))
        })?;
        parse_device_status(response).map_err(DeviceCallFailure::Device)
    }
    pub fn complete_request(&self) -> Result<DeviceCompleteRequest<'_>, DeviceError> {
        self.result
            .complete_request(&self.private_key, &self.code_verifier.0, &self.nonce.0)
    }
    pub fn parse_complete(
        self,
        response: TransportResponse,
        now: u64,
    ) -> Result<(DeviceCredential, DeviceAccessSession), DeviceError> {
        parse_device_complete(
            response,
            &self.result.binding,
            &self.result.device_fingerprint,
            self.private_key,
            now,
        )
    }
    pub fn encode_protected(&self) -> Result<SecretRequestBody, DeviceError> {
        secret_json(&StoredPendingAuthorization {
            schema: "ds-client.device-pending/v1".to_owned(),
            request_id: self.result.request_id.clone(),
            device_code: SecretString(self.result.device_code.0.clone()),
            user_code: self.result.user_code.clone(),
            verification_uri: self.result.verification_uri.clone(),
            expires_at: self.result.expires_at.clone(),
            poll_interval_seconds: self.result.poll_interval_seconds,
            device_fingerprint: self.result.device_fingerprint.clone(),
            scopes: self.result.scopes.clone(),
            binding: self.result.binding.clone(),
            code_verifier: SecretString(self.code_verifier.0.clone()),
            nonce: SecretString(self.nonce.0.clone()),
            private_key: SecretString(URL_SAFE_NO_PAD.encode(self.private_key.0.to_bytes())),
        })
    }
    pub fn decode_protected(bytes: &[u8], profile: &ClientProfile) -> Result<Self, DeviceError> {
        if bytes.len() > 64 * 1024 {
            return Err(DeviceError::Input);
        }
        let mut stored: StoredPendingAuthorization =
            serde_json::from_slice(bytes).map_err(|_| DeviceError::Input)?;
        if stored.schema != "ds-client.device-pending/v1"
            || stored.binding.lane != profile.lane().token()
            || stored.binding.audience != profile.credential_audience_sha256()
        {
            return Err(DeviceError::AuthorityMismatch);
        }
        let private_key = decode_private_key(&mut stored.private_key)?;
        let result = DeviceBeginResult {
            request_id: stored.request_id.clone(),
            device_code: SecretString(stored.device_code.0.clone()),
            user_code: stored.user_code.clone(),
            verification_uri: stored.verification_uri.clone(),
            expires_at: stored.expires_at.clone(),
            poll_interval_seconds: stored.poll_interval_seconds,
            device_fingerprint: stored.device_fingerprint.clone(),
            scopes: stored.scopes.clone(),
            binding: stored.binding.clone(),
        };
        validate_begin_result(&result, &stored.binding)?;
        Self::new(
            result,
            stored.code_verifier.0.clone(),
            stored.nonce.0.clone(),
            private_key,
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeviceStatusResult {
    pub status: DeviceAuthorizationStatus,
    pub expires_at: String,
    pub poll_interval_seconds: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeviceSummary {
    pub device_id: String,
    pub name: String,
    pub platform: String,
    pub fingerprint: String,
    pub principal: DevicePrincipal,
    pub approved_by: DevicePrincipal,
    pub status: String,
    pub scopes: Vec<String>,
    pub binding: DeviceBinding,
    pub approved_at: String,
    pub completed_at: String,
    pub created_at: String,
    pub last_used_at: String,
    pub credential_expires_at: String,
    pub last_refreshed_at: Option<String>,
    pub revoked_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DevicePrincipal {
    pub uid: String,
    pub email: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeviceListResult {
    pub devices: Vec<DeviceSummary>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeviceReadResult {
    pub device: DeviceSummary,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeviceRevokeResult {
    pub device_id: String,
    pub status: String,
    pub revoked: bool,
    pub revoked_at: String,
}

#[derive(Deserialize)]
struct StandardEnvelope<T> {
    data: T,
}

#[derive(Deserialize)]
struct PrincipalWire {
    uid: String,
    email: String,
}

#[derive(Deserialize)]
struct CredentialWire {
    scheme: String,
    device_id: String,
    device_name: String,
    platform: String,
    fingerprint: String,
    principal: PrincipalWire,
    scopes: Vec<String>,
    binding: DeviceBinding,
    credential_expires_at: String,
}

#[derive(Serialize, Deserialize)]
#[serde(transparent)]
struct SecretString(String);
impl SecretString {
    fn take(&mut self) -> String {
        std::mem::take(&mut self.0)
    }
}
impl Drop for SecretString {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

#[derive(Deserialize)]
struct CompleteWire {
    credential: CredentialWire,
    access_token: SecretString,
    token_type: String,
    expires_in: u64,
    expires_at: String,
}

#[derive(Deserialize)]
struct RefreshWire {
    access_token: SecretString,
    token_type: String,
    expires_in: u64,
    expires_at: String,
    credential_expires_at: String,
}

pub fn parse_device_complete(
    response: TransportResponse,
    expected_binding: &DeviceBinding,
    expected_fingerprint: &str,
    private_key: DevicePrivateKey,
    now: u64,
) -> Result<(DeviceCredential, DeviceAccessSession), DeviceError> {
    let mut wire: StandardEnvelope<CompleteWire> = decode_success(response)?;
    validate_access_wire(
        &wire.data.token_type,
        wire.data.expires_in,
        &wire.data.expires_at,
    )?;
    let credential = wire.data.credential;
    if credential.scheme != "DS-Device-Ed25519"
        || credential.scopes.as_slice() != [DEVICE_SCOPE]
        || credential.binding != *expected_binding
        || credential.fingerprint != expected_fingerprint
    {
        return Err(DeviceError::Response);
    }
    let access_token = wire.data.access_token.take();
    let session =
        DeviceAccessSession::new(access_token, now.saturating_add(wire.data.expires_in), now)?;
    let durable = DeviceCredential::new(
        credential.device_id,
        credential.device_name,
        credential.platform,
        credential.fingerprint,
        credential.principal.uid,
        credential.principal.email,
        &credential.binding,
        credential.credential_expires_at,
        private_key,
    )?;
    Ok((durable, session))
}

pub fn parse_device_begin(
    response: TransportResponse,
    expected_binding: &DeviceBinding,
) -> Result<DeviceBeginResult, DeviceError> {
    let result = decode_status::<DeviceBeginResult>(response, 201)?.data;
    validate_begin_result(&result, expected_binding)?;
    Ok(result)
}

pub fn parse_device_status(response: TransportResponse) -> Result<DeviceStatusResult, DeviceError> {
    let result = decode_success::<DeviceStatusResult>(response)?.data;
    bounded(&result.expires_at, 64)?;
    if !(1..=60).contains(&result.poll_interval_seconds) {
        return Err(DeviceError::Response);
    }
    Ok(result)
}

pub fn parse_device_refresh(
    response: TransportResponse,
    now: u64,
) -> Result<(DeviceAccessSession, String), DeviceError> {
    let mut wire: StandardEnvelope<RefreshWire> = decode_success(response)?;
    validate_access_wire(
        &wire.data.token_type,
        wire.data.expires_in,
        &wire.data.expires_at,
    )?;
    bounded(&wire.data.credential_expires_at, 64)?;
    let access_token = wire.data.access_token.take();
    Ok((
        DeviceAccessSession::new(access_token, now.saturating_add(wire.data.expires_in), now)?,
        wire.data.credential_expires_at,
    ))
}

pub fn parse_device_list(response: TransportResponse) -> Result<DeviceListResult, DeviceError> {
    let result = decode_success::<DeviceListResult>(response)?.data;
    if result.devices.len() > 1_000 {
        return Err(DeviceError::Response);
    }
    for device in &result.devices {
        validate_summary(device)?;
    }
    Ok(result)
}
pub fn parse_device_read(response: TransportResponse) -> Result<DeviceReadResult, DeviceError> {
    let result = decode_success::<DeviceReadResult>(response)?.data;
    validate_summary(&result.device)?;
    Ok(result)
}
pub fn parse_device_revoke(response: TransportResponse) -> Result<DeviceRevokeResult, DeviceError> {
    let result = decode_success::<DeviceRevokeResult>(response)?.data;
    bounded(&result.device_id, 128)?;
    bounded(&result.revoked_at, 64)?;
    if result.status != "revoked" || !result.revoked {
        return Err(DeviceError::Response);
    }
    Ok(result)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceProtectedOperation<'a> {
    List,
    Read(&'a str),
    Revoke(&'a str),
}

pub struct DeviceCredential {
    device_id: String,
    device_name: String,
    platform: String,
    fingerprint: String,
    uid: String,
    email: String,
    lane: String,
    credential_audience: String,
    approved_profile_digest: String,
    approved_catalog_digest: String,
    credential_expires_at: String,
    private_key: DevicePrivateKey,
}

/// Non-secret durable identity projection for provider selection and status.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceAuthContext {
    device_id: String,
    device_name: String,
    fingerprint: String,
    uid: String,
    email: String,
    lane: String,
    credential_audience_sha256: String,
    credential_expires_at: String,
}

impl DeviceAuthContext {
    pub fn device_id(&self) -> &str {
        &self.device_id
    }
    pub fn device_name(&self) -> &str {
        &self.device_name
    }
    pub fn fingerprint(&self) -> &str {
        &self.fingerprint
    }
    pub fn uid(&self) -> &str {
        &self.uid
    }
    pub fn email(&self) -> &str {
        &self.email
    }
    pub fn lane(&self) -> &str {
        &self.lane
    }
    pub fn credential_audience_sha256(&self) -> &str {
        &self.credential_audience_sha256
    }
    pub fn credential_expires_at(&self) -> &str {
        &self.credential_expires_at
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredDeviceCredential {
    schema: String,
    device_id: String,
    device_name: String,
    platform: String,
    fingerprint: String,
    uid: String,
    email: String,
    lane: String,
    credential_audience: String,
    approved_profile_digest: String,
    approved_catalog_digest: String,
    credential_expires_at: String,
    private_key: String,
}

impl Drop for StoredDeviceCredential {
    fn drop(&mut self) {
        self.private_key.zeroize();
    }
}

impl DeviceCredential {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        device_id: String,
        device_name: String,
        platform: String,
        fingerprint: String,
        uid: String,
        email: String,
        binding: &DeviceBinding,
        credential_expires_at: String,
        private_key: DevicePrivateKey,
    ) -> Result<Self, DeviceError> {
        bounded(&device_id, 128)?;
        bounded(&device_name, 100)?;
        if !matches!(platform.as_str(), "windows" | "linux" | "macos") {
            return Err(DeviceError::Input);
        }
        exact_prefixed_digest(&fingerprint).map_err(|_| DeviceError::Binding)?;
        bounded(&uid, 256)?;
        bounded(&email, 320)?;
        validate_binding(binding)?;
        bounded(&credential_expires_at, 64)?;
        Ok(Self {
            device_id,
            device_name,
            platform,
            fingerprint,
            uid,
            email,
            lane: binding.lane.clone(),
            credential_audience: binding.audience.clone(),
            approved_profile_digest: binding.profile_digest.clone(),
            approved_catalog_digest: binding.catalog_digest.clone(),
            credential_expires_at,
            private_key,
        })
    }
    pub fn device_id(&self) -> &str {
        &self.device_id
    }
    pub fn device_name(&self) -> &str {
        &self.device_name
    }
    pub fn fingerprint(&self) -> &str {
        &self.fingerprint
    }
    pub fn uid(&self) -> &str {
        &self.uid
    }
    pub fn email(&self) -> &str {
        &self.email
    }
    pub fn credential_expires_at(&self) -> &str {
        &self.credential_expires_at
    }
    pub fn auth_context(&self) -> DeviceAuthContext {
        DeviceAuthContext {
            device_id: self.device_id.clone(),
            device_name: self.device_name.clone(),
            fingerprint: self.fingerprint.clone(),
            uid: self.uid.clone(),
            email: self.email.clone(),
            lane: self.lane.clone(),
            credential_audience_sha256: self.credential_audience.clone(),
            credential_expires_at: self.credential_expires_at.clone(),
        }
    }
    pub fn encode_protected(&self) -> Result<SecretRequestBody, DeviceError> {
        secret_json(&StoredDeviceCredential {
            schema: "ds-client.device-credential/v1".to_owned(),
            device_id: self.device_id.clone(),
            device_name: self.device_name.clone(),
            platform: self.platform.clone(),
            fingerprint: self.fingerprint.clone(),
            uid: self.uid.clone(),
            email: self.email.clone(),
            lane: self.lane.clone(),
            credential_audience: self.credential_audience.clone(),
            approved_profile_digest: self.approved_profile_digest.clone(),
            approved_catalog_digest: self.approved_catalog_digest.clone(),
            credential_expires_at: self.credential_expires_at.clone(),
            private_key: URL_SAFE_NO_PAD.encode(self.private_key.0.to_bytes()),
        })
    }
    pub fn decode_protected(bytes: &[u8], profile: &ClientProfile) -> Result<Self, DeviceError> {
        if bytes.len() > 64 * 1024 {
            return Err(DeviceError::Input);
        }
        let mut stored: StoredDeviceCredential =
            serde_json::from_slice(bytes).map_err(|_| DeviceError::Input)?;
        if stored.schema != "ds-client.device-credential/v1"
            || stored.lane != profile.lane().token()
            || stored.credential_audience != profile.credential_audience_sha256()
        {
            return Err(DeviceError::Binding);
        }
        let mut key_bytes = URL_SAFE_NO_PAD
            .decode(&stored.private_key)
            .map_err(|_| DeviceError::Input)?;
        if key_bytes.len() != 32 {
            key_bytes.zeroize();
            return Err(DeviceError::Input);
        }
        let mut key_array = [0_u8; 32];
        key_array.copy_from_slice(&key_bytes);
        key_bytes.zeroize();
        stored.private_key.zeroize();
        let binding = DeviceBinding {
            lane: stored.lane.clone(),
            audience: stored.credential_audience.clone(),
            profile_digest: stored.approved_profile_digest.clone(),
            catalog_digest: stored.approved_catalog_digest.clone(),
        };
        let key = DevicePrivateKey::from_secret_bytes(key_array);
        key_array.zeroize();
        Self::new(
            stored.device_id.clone(),
            stored.device_name.clone(),
            stored.platform.clone(),
            stored.fingerprint.clone(),
            stored.uid.clone(),
            stored.email.clone(),
            &binding,
            stored.credential_expires_at.clone(),
            key,
        )
    }
    pub fn refresh_request(
        &self,
        timestamp: u64,
        nonce: &str,
        current_binding: &DeviceBinding,
    ) -> Result<DeviceRefreshRequest, DeviceError> {
        if current_binding.lane != self.lane || current_binding.audience != self.credential_audience
        {
            return Err(DeviceError::Binding);
        }
        let approved_binding = DeviceBinding {
            lane: self.lane.clone(),
            audience: self.credential_audience.clone(),
            profile_digest: self.approved_profile_digest.clone(),
            catalog_digest: self.approved_catalog_digest.clone(),
        };
        DeviceRefreshRequest::signed(
            &self.private_key,
            &self.device_id,
            timestamp,
            nonce,
            &approved_binding,
        )
    }
    /// Refresh one short-lived access session, retrying a blink under
    /// `weak_network`. Returns the session and the credential's new expiry.
    ///
    /// `proof` supplies each attempt's `(unix timestamp, nonce)`. ds-brain
    /// accepts a proof nonce exactly once (`DEVICE_NONCE_REPLAYED`), and an
    /// attempt whose request did arrive before the link dropped has spent its
    /// nonce: so every attempt is signed afresh, never resent. A refresh does
    /// not rotate the credential, so a retried one cannot strand it. A host
    /// failure to mint a proof is returned as [`DeviceCallFailure::Host`]
    /// without being retried.
    pub fn refresh_session<T: DeviceTransport, E>(
        &self,
        transport: &mut T,
        current_binding: &DeviceBinding,
        weak_network: &WeakNetwork,
        mut proof: impl FnMut() -> Result<(u64, String), E>,
    ) -> Result<(DeviceAccessSession, String), DeviceCallFailure<E>> {
        let mut signed_at = 0;
        let response = over_weak_network(weak_network, || {
            let (timestamp, nonce) = proof().map_err(DeviceCallFailure::Host)?;
            let request = self
                .refresh_request(timestamp, &nonce, current_binding)
                .map_err(DeviceCallFailure::Device)?;
            let body = secret_json(&request).map_err(DeviceCallFailure::Device)?;
            signed_at = timestamp;
            Ok(transport.refresh(body))
        })?;
        parse_device_refresh(response, signed_at).map_err(DeviceCallFailure::Device)
    }
    pub fn matches_authority(&self, profile: &ClientProfile, expected_uid: &str) -> bool {
        !expected_uid.is_empty()
            && self.uid == expected_uid
            && self.lane == profile.lane().token()
            && self.credential_audience == profile.credential_audience_sha256()
    }
    pub fn authorize_api<'a>(
        &'a self,
        session: &'a DeviceAccessSession,
        profile: &'a ClientProfile,
        expected_uid: &str,
        now: u64,
    ) -> Result<DeviceApiAuthorization<'a>, DeviceError> {
        if !self.matches_authority(profile, expected_uid) {
            return Err(DeviceError::AuthorityMismatch);
        }
        if session.expires_at <= now {
            return Err(DeviceError::AccessToken);
        }
        Ok(DeviceApiAuthorization {
            credential: self,
            session,
            profile,
        })
    }
}

pub struct DeviceAccessSession {
    access_token: String,
    expires_at: u64,
}

impl DeviceAccessSession {
    pub fn new(access_token: String, expires_at: u64, now: u64) -> Result<Self, DeviceError> {
        if access_token.is_empty()
            || access_token.len() > 16 * 1024
            || access_token.trim() != access_token
            || access_token.bytes().any(|byte| byte.is_ascii_control())
            || expires_at <= now
            || expires_at > now.saturating_add(DEVICE_ACCESS_TTL_MAX_SECONDS)
        {
            return Err(DeviceError::AccessToken);
        }
        Ok(Self {
            access_token,
            expires_at,
        })
    }
    pub fn expires_at(&self) -> u64 {
        self.expires_at
    }
    pub fn protected_call<'a>(
        &'a self,
        operation: DeviceProtectedOperation<'a>,
    ) -> DeviceProtectedCall<'a> {
        DeviceProtectedCall {
            session: self,
            operation,
        }
    }
}

impl Drop for DeviceAccessSession {
    fn drop(&mut self) {
        self.access_token.zeroize();
    }
}

/// Privileged host input for existing fixed typed API transports.
///
/// It is not an operation result and has no serialization or Debug surface.
/// The host may read the bearer only while executing one already-typed call.
pub struct DeviceApiAuthorization<'a> {
    credential: &'a DeviceCredential,
    session: &'a DeviceAccessSession,
    profile: &'a ClientProfile,
}

impl DeviceApiAuthorization<'_> {
    pub fn bearer_token(&self) -> &str {
        &self.session.access_token
    }
    pub fn uid(&self) -> &str {
        &self.credential.uid
    }
    pub fn email(&self) -> &str {
        &self.credential.email
    }
    pub fn device_id(&self) -> &str {
        &self.credential.device_id
    }

    fn authority(&self) -> crate::authorized::Authority<'_> {
        crate::authorized::Authority {
            bearer_token: &self.session.access_token,
            uid: &self.credential.uid,
            email: &self.credential.email,
            device_id: Some(&self.credential.device_id),
        }
    }

    fn ensure_active(&self, now: u64) -> Result<(), ClientError> {
        if self.session.expires_at <= now {
            Err(ClientError::new(
                ErrorKind::SignedOut,
                "the memory-only device access session expired and must be refreshed",
            ))
        } else {
            Ok(())
        }
    }

    /// Execute a closed Sync Center operation using this linked device.
    pub fn sync_gateway<T: AuthorizedTransport>(
        &self,
        transport: &mut T,
        request: &crate::NativeSyncRequest,
        now: u64,
    ) -> Result<serde_json::Value, ClientError> {
        self.ensure_active(now)?;
        crate::authorized::sync_gateway(self.profile, transport, self.authority(), request)
    }

    pub fn list_projects<T: AuthorizedTransport>(
        &self,
        transport: &mut T,
        now: u64,
    ) -> Result<ProjectDirectory, ClientError> {
        self.ensure_active(now)?;
        crate::authorized::list_projects(self.profile, transport, self.authority())
    }

    pub fn feeder_configuration<T: AuthorizedTransport>(
        &self,
        transport: &mut T,
        project: &str,
        bounds: Option<&crate::ProjectConfigurationChange>,
        now: u64,
    ) -> Result<crate::FeederConfiguration, ClientError> {
        crate::authorized::validate_project_report(project)?;
        self.ensure_active(now)?;
        crate::authorized::feeder_configuration(
            self.profile,
            transport,
            self.authority(),
            project,
            bounds,
        )
    }

    /// Read exact persisted analysis bytes using this captured device authority
    /// and an explicit project/head fence. Server refusals retain their codes.
    // Keep project, transformer, version and content digest explicit at this
    // existing authenticated boundary rather than changing its public API.
    #[allow(clippy::too_many_arguments)]
    pub fn transformer_analysis<T: AuthorizedTransport>(
        &self,
        transport: &mut T,
        project: &str,
        transformer: &str,
        analysis_version: u64,
        content_digest: &str,
        analysis_sha256: &str,
        now: u64,
    ) -> Result<Vec<u8>, ClientError> {
        crate::transformer_analysis::validate(
            project,
            transformer,
            analysis_version,
            content_digest,
            analysis_sha256,
        )?;
        self.ensure_active(now)?;
        crate::transformer_analysis::read(
            self.profile,
            transport,
            self.authority(),
            project,
            transformer,
            analysis_version,
            content_digest,
            analysis_sha256,
        )
    }

    pub fn transformer_context<T: AuthorizedTransport>(
        &self,
        transport: &mut T,
        ds_project: &str,
        transformer: &str,
        now: u64,
    ) -> Result<TransformerContext, ClientError> {
        crate::authorized::validate_transformer_context(ds_project, transformer)?;
        self.ensure_active(now)?;
        crate::authorized::transformer_context(
            self.profile,
            transport,
            self.authority(),
            ds_project,
            transformer,
            crate::TransformerContextProjection::Full,
        )
    }

    /// Read persisted transformer layers for report integrity verification.
    /// Ordinary reads continue to use the governed full projection.
    pub fn transformer_context_saved<T: AuthorizedTransport>(
        &self,
        transport: &mut T,
        ds_project: &str,
        transformer: &str,
        now: u64,
    ) -> Result<TransformerContext, ClientError> {
        crate::authorized::validate_transformer_context(ds_project, transformer)?;
        self.ensure_active(now)?;
        crate::authorized::transformer_context(
            self.profile,
            transport,
            self.authority(),
            ds_project,
            transformer,
            crate::TransformerContextProjection::Saved,
        )
    }

    pub fn project_forms<T: AuthorizedTransport>(
        &self,
        transport: &mut T,
        ds_project: &str,
        now: u64,
    ) -> Result<ProjectFormsSnapshot, ClientError> {
        crate::authorized::validate_project_forms(ds_project)?;
        self.ensure_active(now)?;
        crate::authorized::project_forms(self.profile, transport, self.authority(), ds_project)
    }

    pub fn project_form_editor<T: AuthorizedTransport>(
        &self,
        transport: &mut T,
        ds_project: &str,
        form_slug: &str,
        now: u64,
    ) -> Result<ProjectFormSettingsEditor, ClientError> {
        crate::authorized::validate_project_form_editor(ds_project, form_slug)?;
        self.ensure_active(now)?;
        crate::authorized::project_form_editor(
            self.profile,
            transport,
            self.authority(),
            ds_project,
            form_slug,
        )
    }

    pub fn solar_snapshot<T: AuthorizedTransport>(
        &self,
        transport: &mut T,
        ds_project: &str,
        template_id: &str,
        now: u64,
    ) -> Result<SolarSnapshot, ClientError> {
        crate::authorized::validate_solar_snapshot(ds_project, template_id)?;
        self.ensure_active(now)?;
        crate::authorized::solar_snapshot(
            self.profile,
            transport,
            self.authority(),
            ds_project,
            template_id,
        )
    }

    pub fn survey_query<T: AuthorizedTransport>(
        &self,
        transport: &mut T,
        ds_project: &str,
        query: &SurveyQueryRequest,
        now: u64,
    ) -> Result<SurveyQueryResult, ClientError> {
        crate::authorized::validate_survey_query(ds_project)?;
        self.ensure_active(now)?;
        crate::authorized::survey_query(
            self.profile,
            transport,
            self.authority(),
            ds_project,
            query,
        )
    }

    pub fn survey_entries_select<T: AuthorizedTransport>(
        &self,
        transport: &mut T,
        ds_project: &str,
        request: &SurveyEntriesSelectRequest,
        now: u64,
    ) -> Result<SurveyEntriesSelection, ClientError> {
        crate::authorized::validate_survey_entries_select(ds_project)?;
        self.ensure_active(now)?;
        crate::authorized::survey_entries_select(
            self.profile,
            transport,
            self.authority(),
            ds_project,
            request,
        )
    }

    pub fn survey_entries_changes<T: AuthorizedTransport>(
        &self,
        transport: &mut T,
        ds_project: &str,
        request: &SurveyEntriesChangesRequest,
        now: u64,
    ) -> Result<SurveyEntriesChanges, ClientError> {
        crate::authorized::validate_survey_entries_changes(ds_project)?;
        self.ensure_active(now)?;
        crate::authorized::survey_entries_changes(
            self.profile,
            transport,
            self.authority(),
            ds_project,
            request,
        )
    }

    pub fn survey_delete<T: AuthorizedTransport>(
        &self,
        transport: &mut T,
        project: &str,
        request: &crate::SurveyDeleteRequest,
        now: u64,
    ) -> Result<serde_json::Value, ClientError> {
        if request.project() != project {
            return Err(ClientError::new(
                ErrorKind::InvalidInput,
                "Survey delete project mismatch",
            ));
        }
        crate::authorized::validate_survey_entry_create(project)?;
        self.ensure_active(now)?;
        crate::survey_delete::run(self.profile, transport, self.authority(), project, request)
    }

    pub fn survey_entry_create<T: AuthorizedTransport>(
        &self,
        transport: &mut T,
        project_id: &str,
        request: &SurveyEntryCreateRequest,
        now: u64,
    ) -> Result<SurveyEntryCreateReceipt, ClientError> {
        crate::authorized::validate_survey_entry_create(project_id)?;
        self.ensure_active(now)?;
        crate::authorized::survey_entry_create(
            self.profile,
            transport,
            self.authority(),
            project_id,
            request,
        )
    }

    pub fn tile_list<T: AuthorizedTransport>(
        &self,
        transport: &mut T,
        ds_project: &str,
        include_global: bool,
        now: u64,
    ) -> Result<crate::TileCatalog, ClientError> {
        crate::authorized::validate_tiles(ds_project)?;

        self.ensure_active(now)?;
        crate::authorized::tile_list(
            self.profile,
            transport,
            self.authority(),
            ds_project,
            include_global,
        )
    }

    pub fn tile_add<T: AuthorizedTransport>(
        &self,
        transport: &mut T,
        ds_project: &str,
        tile_type: TileType,
        source_project: &str,
        now: u64,
    ) -> Result<crate::TileMutation, ClientError> {
        crate::authorized::validate_tiles(ds_project)?;
        crate::tiles::validate_catalog_id(source_project)?;
        self.ensure_active(now)?;
        crate::authorized::tile_add(
            self.profile,
            transport,
            self.authority(),
            ds_project,
            tile_type,
            source_project,
        )
    }

    pub fn tile_remove<T: AuthorizedTransport>(
        &self,
        transport: &mut T,
        ds_project: &str,
        tile_id: &str,
        scope: crate::TileScope,
        now: u64,
    ) -> Result<crate::TileMutation, ClientError> {
        crate::authorized::validate_tiles(ds_project)?;
        crate::tiles::validate_catalog_id(tile_id)?;
        self.ensure_active(now)?;
        crate::authorized::tile_remove(
            self.profile,
            transport,
            self.authority(),
            ds_project,
            tile_id,
            scope,
        )
    }

    pub fn design_selections<T: AuthorizedTransport>(
        &self,
        transport: &mut T,
        ds_project: &str,
        request: &crate::DesignSelectionRequest,
        now: u64,
    ) -> Result<crate::DesignSelectionAnswer, ClientError> {
        crate::authorized::validate_tiles(ds_project)?;
        request.validate()?;
        self.ensure_active(now)?;
        crate::authorized::design_selections(
            self.profile,
            transport,
            self.authority(),
            ds_project,
            request,
        )
    }
    /// One account-scoped messaging or notification action, authorized under
    /// this protected device principal by the same owner as a user session.
    pub fn messaging<T: AuthorizedTransport>(
        &self,
        transport: &mut T,
        command: &crate::messaging::Command,
        now: u64,
    ) -> Result<serde_json::Value, ClientError> {
        self.ensure_active(now)?;
        crate::messaging::run(self.profile, transport, self.authority(), command)
    }
    /// One governed correspondence action, on a device session — the same
    /// door as [`Self::project_management`].
    pub fn project_correspondence<T: AuthorizedTransport>(
        &self,
        transport: &mut T,
        ds_project: &str,
        action: &crate::project_correspondence::Action,
        now: u64,
    ) -> Result<serde_json::Value, ClientError> {
        crate::authorized::validate_tiles(ds_project)?;
        self.ensure_active(now)?;
        crate::project_correspondence::run(
            self.profile,
            transport,
            self.authority(),
            ds_project,
            action,
        )
    }
    /// One governed project-management read, on a device session.
    ///
    /// The same door the restored-user path takes; only the credential differs.
    /// A headless box linked by device must read a plan too, and `ds pm` must
    /// answer identically whichever credential carried it.
    pub fn project_management<T: AuthorizedTransport>(
        &self,
        transport: &mut T,
        ds_project: &str,
        command: &crate::project_management::Command,
        now: u64,
    ) -> Result<serde_json::Value, ClientError> {
        if !matches!(
            command,
            crate::project_management::Command::PublicProjects { .. }
        ) {
            crate::authorized::validate_tiles(ds_project)?;
        }
        self.ensure_active(now)?;
        crate::project_management::run(
            self.profile,
            transport,
            self.authority(),
            ds_project,
            command,
        )
    }
    pub fn member_form_grant<T: AuthorizedTransport>(
        &self,
        transport: &mut T,
        ds_project: &str,
        command: &crate::member_form_grants::Request,
        now: u64,
    ) -> Result<crate::member_form_grants::GrantResult, ClientError> {
        crate::authorized::validate_tiles(ds_project)?;
        self.ensure_active(now)?;
        crate::member_form_grants::run(
            self.profile,
            transport,
            self.authority(),
            ds_project,
            command,
        )
    }
    pub fn data_distribution<T: AuthorizedTransport>(
        &self,
        transport: &mut T,
        ds_project: &str,
        request: &crate::DataDistributionRequest,
        now: u64,
    ) -> Result<serde_json::Value, ClientError> {
        crate::authorized::validate_tiles(ds_project)?;
        request.validate()?;
        self.ensure_active(now)?;
        crate::authorized::data_distribution(
            self.profile,
            transport,
            self.authority(),
            ds_project,
            request,
        )
    }
    pub fn printing<T: AuthorizedTransport>(
        &self,
        transport: &mut T,
        ds_project: &str,
        request: &crate::PrintingRequest,
        now: u64,
    ) -> Result<serde_json::Value, ClientError> {
        crate::authorized::validate_tiles(ds_project)?;
        request.validate_for(ds_project)?;
        self.ensure_active(now)?;
        crate::authorized::printing(
            self.profile,
            transport,
            self.authority(),
            ds_project,
            request,
        )
    }
    /// Read only the explicit project's governed standard and A4 documents.
    pub fn printing_standard<T: AuthorizedTransport>(
        &self,
        transport: &mut T,
        ds_project: &str,
        request: &crate::PrintingStandardRequest,
        now: u64,
    ) -> Result<serde_json::Value, ClientError> {
        crate::authorized::validate_tiles(ds_project)?;
        request.validate()?;
        self.ensure_active(now)?;
        crate::printing_standard::read(
            self.profile,
            transport,
            self.authority(),
            ds_project,
            request,
        )
    }
    /// Plan, census or apply one copy of a project's network documents
    /// (`ds_project` is the destination).
    pub fn network_documents<T: AuthorizedTransport>(
        &self,
        transport: &mut T,
        ds_project: &str,
        request: &crate::NetworkDocumentsRequest,
        now: u64,
    ) -> Result<serde_json::Value, ClientError> {
        request.validate_for(ds_project)?;
        self.ensure_active(now)?;
        crate::network_documents::call(
            self.profile,
            transport,
            self.authority(),
            ds_project,
            request,
        )
    }
    /// Plan, census or apply the governed printing-template migration of one
    /// library (`ds_project` empty for the global one).
    pub fn print_style_keys<T: AuthorizedTransport>(
        &self,
        transport: &mut T,
        ds_project: &str,
        request: &crate::PrintStyleKeysRequest,
        now: u64,
    ) -> Result<serde_json::Value, ClientError> {
        request.validate_for(ds_project)?;
        self.ensure_active(now)?;
        crate::printing_style_keys::call(
            self.profile,
            transport,
            self.authority(),
            ds_project,
            request,
        )
    }
    pub fn layer_config<T: AuthorizedTransport>(
        &self,
        transport: &mut T,
        ds_project: &str,
        refresh: bool,
        now: u64,
    ) -> Result<crate::LayerSnapshot, ClientError> {
        crate::authorized::validate_tiles(ds_project)?;
        self.ensure_active(now)?;
        crate::authorized::layer_config(
            self.profile,
            transport,
            self.authority(),
            ds_project,
            refresh,
        )
    }
    pub fn style_catalog<T: AuthorizedTransport>(
        &self,
        transport: &mut T,
        ds_project: &str,
        now: u64,
    ) -> Result<crate::StyleSnapshot, ClientError> {
        crate::authorized::validate_tiles(ds_project)?;
        self.ensure_active(now)?;
        crate::authorized::style_catalog(self.profile, transport, self.authority(), ds_project)
    }
    pub fn layer_reorder<T: AuthorizedTransport>(
        &self,
        transport: &mut T,
        ds_project: &str,
        orders: &[crate::LayerOrder],
        now: u64,
    ) -> Result<crate::LayerOrderReceipt, ClientError> {
        crate::authorized::validate_tiles(ds_project)?;
        crate::layers::validate_orders(orders)?;
        self.ensure_active(now)?;
        crate::authorized::layer_reorder(
            self.profile,
            transport,
            self.authority(),
            ds_project,
            orders,
        )
    }
    pub fn layer_default_visibility<T: AuthorizedTransport>(
        &self,
        transport: &mut T,
        ds_project: &str,
        defaults: &[crate::LayerVisibilityDefault],
        now: u64,
    ) -> Result<crate::LayerVisibilityDefaultReceipt, ClientError> {
        crate::authorized::validate_tiles(ds_project)?;
        crate::layers::validate_visibility_defaults(defaults)?;
        self.ensure_active(now)?;
        crate::authorized::layer_default_visibility(
            self.profile,
            transport,
            self.authority(),
            ds_project,
            defaults,
        )
    }
    pub fn project_data<T: AuthorizedTransport>(
        &self,
        transport: &mut T,
        project: &str,
        command: crate::ProjectDataCommand<'_>,
        now: u64,
    ) -> Result<crate::ProjectDataReceipt, ClientError> {
        command.validate()?;
        crate::authorized::validate_tiles(project)?;
        self.ensure_active(now)?;
        crate::authorized::project_data(self.profile, transport, self.authority(), project, command)
    }
    pub fn status_processing<T: AuthorizedTransport>(
        &self,
        transport: &mut T,
        project: &str,
        command: crate::StatusProcessingCommand<'_>,
        now: u64,
    ) -> Result<crate::StatusProcessingReceipt, ClientError> {
        crate::authorized::validate_tiles(project)?;
        self.ensure_active(now)?;
        crate::authorized::status_processing(
            self.profile,
            transport,
            self.authority(),
            project,
            command,
        )
    }
    pub fn survey_photo<T: AuthorizedTransport>(
        &self,
        transport: &mut T,
        project: &str,
        command: crate::survey_photo::Command<'_>,
        now: u64,
    ) -> Result<crate::survey_photo::Receipt, ClientError> {
        crate::authorized::validate_tiles(project)?;
        self.ensure_active(now)?;
        crate::survey_photo::run(self.profile, transport, self.authority(), project, command)
    }
    pub fn repair_transformer<T:AuthorizedTransport>(&self,transport:&mut T,project:&str,input:&crate::TransformerRepairPublication,now:u64)->Result<serde_json::Value,ClientError> {
        input.validate(project)?;
        self.ensure_active(now)?;
        crate::transformer_repair::run(self.profile,transport,self.authority(),project,input)
    }
    pub fn save_transformers<T: AuthorizedTransport>(
        &self,
        transport: &mut T,
        project: &str,
        batch: &crate::TransformerSaveBatch,
        now: u64,
    ) -> Result<crate::TransformerSaveReceipt, ClientError> {
        batch.validate(project)?;
        self.ensure_active(now)?;
        crate::transformer_save::run(self.profile, transport, self.authority(), project, batch)
    }
    pub fn feedback<T: AuthorizedTransport>(
        &self,
        transport: &mut T,
        command: &crate::feedback::Command,
        now: u64,
    ) -> Result<serde_json::Value, ClientError> {
        command.validate()?;
        self.ensure_active(now)?;
        crate::feedback::run(self.profile, transport, self.authority(), command)
    }
    /// The projectless governance doors, on a device session.
    ///
    /// Governance is server-side: ds-brain's `Authentication` middleware admits
    /// a device credential on `/api/v1/governance` and `/sre/*` exactly as it
    /// admits a restored user, and `platform.admin`/`app.admin` on the
    /// principal decides the answer either way. Until these existed, a host
    /// linked by device could read its projects but not the estate — `ds
    /// install list` refused `headless_signed_out` on a box whose `ds auth
    /// status` said signed in. Each delegates to the same `run` the native
    /// [`crate::Client`] twin uses; only the credential differs.
    pub fn installs<T: AuthorizedTransport>(
        &self,
        transport: &mut T,
        command: &crate::installs::Command,
        now: u64,
    ) -> Result<serde_json::Value, ClientError> {
        command.validate()?;
        self.ensure_active(now)?;
        crate::installs::run(self.profile, transport, self.authority(), command)
    }
    /// Create a project or edit its properties on a device session.
    ///
    /// ds-brain's `Authentication` middleware admits a device credential on
    /// `/api/v1/projects` exactly as it admits a restored user (the principal's
    /// email and uid are set either way, and the capability gate reads the
    /// email), so a host linked by device administers projects too.
    pub fn project_properties<T: AuthorizedTransport>(
        &self,
        transport: &mut T,
        command: &crate::project_properties::Command,
        now: u64,
    ) -> Result<crate::project_properties::Receipt, ClientError> {
        command.validate()?;
        self.ensure_active(now)?;
        crate::project_properties::run(self.profile, transport, self.authority(), command)
    }
    pub fn sre<T: AuthorizedTransport>(
        &self,
        transport: &mut T,
        command: &crate::sre::Command,
        now: u64,
    ) -> Result<serde_json::Value, ClientError> {
        command.validate()?;
        self.ensure_active(now)?;
        crate::sre::run(self.profile, transport, self.authority(), command)
    }
    pub fn survey_entries_read<T: AuthorizedTransport>(
        &self,
        transport: &mut T,
        project: &str,
        request: &crate::SurveyEntriesReadRequest,
        now: u64,
    ) -> Result<crate::SurveyEntriesRead, ClientError> {
        self.ensure_active(now)?;
        crate::survey_entries_read::run(self.profile, transport, self.authority(), project, request)
    }
    pub fn survey_media_grant<T: AuthorizedTransport>(
        &self,
        transport: &mut T,
        project: &str,
        others: &[&str],
        now: u64,
    ) -> Result<crate::MediaGrants, ClientError> {
        self.ensure_active(now)?;
        crate::survey_media_fetch::grant(self.profile, transport, self.authority(), project, others)
    }
    /// The grant a delivered report's photo links are written under: the
    /// named project, ds-brain's report default life.
    pub fn report_media_grant<T: AuthorizedTransport>(
        &self,
        transport: &mut T,
        project: &str,
        now: u64,
    ) -> Result<crate::MediaGrants, ClientError> {
        self.ensure_active(now)?;
        crate::survey_media_fetch::grant_for(
            self.profile,
            transport,
            self.authority(),
            project,
            &[],
            crate::survey_media_fetch::GrantUse::ReportLinks,
        )
    }
    pub fn survey_entries_query<T: AuthorizedTransport>(
        &self,
        transport: &mut T,
        project: &str,
        body: &serde_json::Value,
        now: u64,
    ) -> Result<Vec<u8>, ClientError> {
        self.ensure_active(now)?;
        crate::survey_entries_read::query(self.profile, transport, self.authority(), project, body)
    }
    pub fn admin_bounds<T: AuthorizedTransport>(
        &self,
        transport: &mut T,
        command: &crate::admin_bounds::Command,
        now: u64,
    ) -> Result<crate::admin_bounds::Answer, ClientError> {
        command.validate()?;
        self.ensure_active(now)?;
        crate::admin_bounds::run(self.profile, transport, self.authority(), command)
    }
    pub fn grid_catalog<T: AuthorizedTransport>(
        &self,
        transport: &mut T,
        command: &crate::grid_catalog::Command,
        now: u64,
    ) -> Result<serde_json::Value, ClientError> {
        command.validate()?;
        self.ensure_active(now)?;
        crate::grid_catalog::run(self.profile, transport, self.authority(), command)
    }
    pub fn global_tiles<T: AuthorizedTransport>(
        &self,
        transport: &mut T,
        command: &crate::global_tiles::Command,
        now: u64,
    ) -> Result<serde_json::Value, ClientError> {
        command.validate()?;
        self.ensure_active(now)?;
        crate::global_tiles::run(self.profile, transport, self.authority(), command)
    }
    pub fn shared_assets<T: AuthorizedTransport>(
        &self,
        transport: &mut T,
        project: &str,
        command: &crate::shared_assets::Command,
        now: u64,
    ) -> Result<serde_json::Value, ClientError> {
        command.validate(project)?;
        self.ensure_active(now)?;
        crate::shared_assets::run(self.profile, transport, self.authority(), project, command)
    }
    /// One governed catalogue action on the project's assets, on a device
    /// session. The same door the restored-user path takes, including index forms.
    pub fn project_assets<T: AuthorizedTransport>(
        &self,
        transport: &mut T,
        project: &str,
        command: &crate::project_assets::Command,
        reader: Option<&mut dyn std::io::Read>,
        now: u64,
    ) -> Result<serde_json::Value, ClientError> {
        command.validate(project)?;
        self.ensure_active(now)?;
        crate::project_assets::run(
            self.profile,
            transport,
            self.authority(),
            project,
            command,
            reader,
        )
    }
    /// One governed design annotation action, on a device session.
    pub fn design_annotations<T: AuthorizedTransport>(
        &self,
        transport: &mut T,
        project: &str,
        command: &crate::design_annotations::Command,
        now: u64,
    ) -> Result<serde_json::Value, ClientError> {
        command.validate(project)?;
        self.ensure_active(now)?;
        crate::design_annotations::run(self.profile, transport, self.authority(), project, command)
    }
    /// The project's known-column visibility, on a device session.
    pub fn known_columns<T: AuthorizedTransport>(
        &self,
        transport: &mut T,
        project: &str,
        command: &crate::known_columns::Command,
        now: u64,
    ) -> Result<serde_json::Value, ClientError> {
        command.validate(project)?;
        self.ensure_active(now)?;
        crate::known_columns::run(self.profile, transport, self.authority(), project, command)
    }
    /// One material propagation through the report gate, on a device session.
    pub fn material_propagation<T: AuthorizedTransport>(
        &self,
        transport: &mut T,
        project: &str,
        request: &serde_json::Value,
        now: u64,
    ) -> Result<serde_json::Value, ClientError> {
        crate::authorized::validate_project_report(project)?;
        self.ensure_active(now)?;
        crate::authorized::material_propagation(
            self.profile,
            transport,
            self.authority(),
            project,
            request,
        )
    }
    /// One asset's row and its verified bytes, on a device session.
    pub fn read_asset_bytes<T: AuthorizedTransport>(
        &self,
        transport: &mut T,
        project: &str,
        asset_id: &str,
        now: u64,
    ) -> Result<(serde_json::Value, Vec<u8>), ClientError> {
        self.ensure_active(now)?;
        crate::project_assets::read_bytes(
            self.profile,
            transport,
            self.authority(),
            project,
            asset_id,
        )
    }
    pub fn design_versions<T: AuthorizedTransport>(
        &self,
        transport: &mut T,
        project: &str,
        command: &crate::design_versions::Command,
        now: u64,
    ) -> Result<serde_json::Value, ClientError> {
        command.validate(project).map_err(|_| {
            ClientError::new(
                crate::ErrorKind::InvalidInput,
                "invalid published version request; use distinct vN IDs or --to head",
            )
        })?;
        self.ensure_active(now)?;
        crate::design_versions::run(self.profile, transport, self.authority(), project, command)
    }
    pub fn design_attachments<T: AuthorizedTransport>(
        &self,
        transport: &mut T,
        project: &str,
        command: &crate::design_attachments::Command,
        now: u64,
    ) -> Result<serde_json::Value, ClientError> {
        command.validate(project)?;
        self.ensure_active(now)?;
        crate::design_attachments::run(self.profile, transport, self.authority(), project, command)
    }
    /// One attachment revision's receipt and its verified bytes, on a device
    /// session.
    pub fn design_attachment_bytes<T: AuthorizedTransport>(
        &self,
        transport: &mut T,
        project: &str,
        attachment: &str,
        revision: Option<&str>,
        now: u64,
    ) -> Result<(serde_json::Value, Vec<u8>), ClientError> {
        crate::design_attachments::Command::Download {
            attachment: attachment.to_owned(),
            revision: revision.map(str::to_owned),
        }
        .validate(project)?;
        self.ensure_active(now)?;
        crate::design_attachments::fetch(
            self.profile,
            transport,
            self.authority(),
            project,
            attachment,
            revision,
        )
    }
    pub fn design_tags<T: AuthorizedTransport>(
        &self,
        transport: &mut T,
        project: &str,
        command: &crate::design_tags::Command,
        now: u64,
    ) -> Result<serde_json::Value, ClientError> {
        command.validate(project)?;
        self.ensure_active(now)?;
        crate::design_tags::run(self.profile, transport, self.authority(), project, command)
    }
    pub fn design_migration<T: AuthorizedTransport>(
        &self,
        transport: &mut T,
        project: &str,
        command: &crate::design_migration::Command,
        now: u64,
    ) -> Result<serde_json::Value, ClientError> {
        command.validate(project)?;
        self.ensure_active(now)?;
        crate::design_migration::run(self.profile, transport, self.authority(), project, command)
    }
    /// Survey data INTO `project` from the command's source, both explicit.
    pub fn survey_migration<T: AuthorizedTransport>(
        &self,
        transport: &mut T,
        project: &str,
        command: &crate::survey_migration::Command,
        now: u64,
    ) -> Result<serde_json::Value, ClientError> {
        command.validate(project)?;
        self.ensure_active(now)?;
        crate::survey_migration::run(self.profile, transport, self.authority(), project, command)
    }
    pub fn report_artifact<T: AuthorizedTransport>(
        &self,
        transport: &mut T,
        project: &str,
        command: &crate::report_artifact::Command,
        now: u64,
    ) -> Result<serde_json::Value, ClientError> {
        command.validate(project)?;
        self.ensure_active(now)?;
        crate::report_artifact::run(self.profile, transport, self.authority(), project, command)
    }
    pub fn remove_report_artifact<T: AuthorizedTransport>(
        &self,
        transport: &mut T,
        project: &str,
        command: &crate::report_artifact::RemoveCommand,
        now: u64,
    ) -> Result<serde_json::Value, ClientError> {
        command.validate(project)?;
        self.ensure_active(now)?;
        crate::report_artifact::remove(self.profile, transport, self.authority(), project, command)
    }
    pub fn grid_models<T: AuthorizedTransport>(
        &self,
        transport: &mut T,
        project: &str,
        command: &crate::grid_models::Command,
        now: u64,
    ) -> Result<crate::grid_models::Receipt, ClientError> {
        command.validate(project)?;
        self.ensure_active(now)?;
        crate::grid_models::run(self.profile, transport, self.authority(), project, command)
    }
    pub fn solar_project<T: AuthorizedTransport>(
        &self,
        transport: &mut T,
        project: &str,
        command: &crate::solar_project::Command,
        now: u64,
    ) -> Result<serde_json::Value, ClientError> {
        command.validate(project)?;
        self.ensure_active(now)?;
        crate::solar_project::run(self.profile, transport, self.authority(), project, command)
    }
    pub fn survey_control<T: AuthorizedTransport>(
        &self,
        transport: &mut T,
        project: Option<&str>,
        command: &crate::SurveyControlCommand,
        now: u64,
    ) -> Result<serde_json::Value, ClientError> {
        command.validate()?;
        self.ensure_active(now)?;
        crate::survey_control_run::run(self.profile, transport, self.authority(), project, command)
    }
    pub fn style_edit<T: AuthorizedTransport>(
        &self,
        transport: &mut T,
        ds_project: &str,
        reference: &str,
        instruction: &crate::StyleInstruction,
        apply: bool,
        now: u64,
    ) -> Result<crate::StyleEditReceipt, ClientError> {
        crate::authorized::validate_tiles(ds_project)?;
        self.ensure_active(now)?;
        crate::authorized::style_edit(
            self.profile,
            transport,
            self.authority(),
            ds_project,
            reference,
            instruction,
            apply,
        )
    }
    pub fn style_history<T: AuthorizedTransport>(
        &self,
        transport: &mut T,
        ds_project: &str,
        command: &ds_command_kernel::style_history::Command,
        now: u64,
    ) -> Result<serde_json::Value, ClientError> {
        crate::authorized::validate_tiles(ds_project)?;
        self.ensure_active(now)?;
        crate::styles::history(
            self.profile,
            transport,
            self.authority(),
            ds_project,
            command,
        )
    }
    pub fn style_governance<T: AuthorizedTransport>(
        &self,
        transport: &mut T,
        ds_project: &str,
        command: &ds_command_kernel::style_governance::Command,
        now: u64,
    ) -> Result<serde_json::Value, ClientError> {
        crate::authorized::validate_tiles(ds_project)?;
        self.ensure_active(now)?;
        crate::styles::governance(
            self.profile,
            transport,
            self.authority(),
            ds_project,
            command,
        )
    }
    pub fn tile_status<T: AuthorizedTransport>(
        &self,
        transport: &mut T,
        ds_project: &str,
        tile_type: TileType,
        now: u64,
    ) -> Result<TileOperationResult, ClientError> {
        crate::authorized::validate_tiles(ds_project)?;
        self.ensure_active(now)?;
        crate::authorized::tile_status(
            self.profile,
            transport,
            self.authority(),
            ds_project,
            tile_type,
        )
    }

    pub fn tile_preflight<T: AuthorizedTransport>(
        &self,
        transport: &mut T,
        ds_project: &str,
        tile_type: TileType,
        now: u64,
    ) -> Result<TilePreflight, ClientError> {
        crate::authorized::validate_tiles(ds_project)?;
        self.ensure_active(now)?;
        crate::authorized::tile_preflight(
            self.profile,
            transport,
            self.authority(),
            ds_project,
            tile_type,
        )
    }

    pub fn tile_generate<T: AuthorizedTransport>(
        &self,
        transport: &mut T,
        ds_project: &str,
        tile_type: TileType,
        force: bool,
        now: u64,
    ) -> Result<TileOperationResult, ClientError> {
        crate::authorized::validate_tiles(ds_project)?;
        self.ensure_active(now)?;
        crate::authorized::tile_generate(
            self.profile,
            transport,
            self.authority(),
            ds_project,
            tile_type,
            force,
        )
    }

    pub fn compounded_report<T: AuthorizedTransport>(
        &self,
        transport: &mut T,
        ds_project: &str,
        request: &CompoundedReportRequest,
        now: u64,
    ) -> Result<CompoundedReportReceipt, ClientError> {
        crate::authorized::validate_project_report(ds_project)?;
        self.ensure_active(now)?;
        crate::authorized::compounded_report(
            self.profile,
            transport,
            self.authority(),
            ds_project,
            request,
        )
    }

    pub fn export_reports<T: AuthorizedTransport>(
        &self,
        transport: &mut T,
        ds_project: &str,
        request: &TransformerSet,
        now: u64,
    ) -> Result<crate::project_report::ExportReportsReceipt, ClientError> {
        crate::authorized::validate_project_report(ds_project)?;
        self.ensure_active(now)?;
        crate::authorized::export_reports(
            self.profile,
            transport,
            self.authority(),
            ds_project,
            request,
        )
    }

    pub fn compounded_report_list<T: AuthorizedTransport>(
        &self,
        transport: &mut T,
        ds_project: &str,
        now: u64,
    ) -> Result<CompoundedArchiveCatalog, ClientError> {
        crate::authorized::validate_project_report(ds_project)?;
        self.ensure_active(now)?;
        crate::authorized::compounded_report_list(
            self.profile,
            transport,
            self.authority(),
            ds_project,
        )
    }

    pub fn transformer_inventory<T: AuthorizedTransport>(
        &self,
        transport: &mut T,
        ds_project: &str,
        requested: &TransformerSet,
        now: u64,
    ) -> Result<TransformerInventory, ClientError> {
        crate::authorized::validate_project_report(ds_project)?;
        self.ensure_active(now)?;
        crate::authorized::transformer_inventory(
            self.profile,
            transport,
            self.authority(),
            ds_project,
            requested,
        )
    }

    pub fn transformer_status<T: AuthorizedTransport>(
        &self,
        transport: &mut T,
        ds_project: &str,
        requested: &TransformerSet,
        now: u64,
    ) -> Result<TransformerStatusList, ClientError> {
        crate::authorized::validate_project_report(ds_project)?;
        self.ensure_active(now)?;
        crate::authorized::transformer_status(
            self.profile,
            transport,
            self.authority(),
            ds_project,
            requested,
        )
    }

    pub fn transformer_retirement<T: AuthorizedTransport>(
        &self,
        transport: &mut T,
        ds_project: &str,
        action: RetirementAction,
        request: &RetirementRequest,
        now: u64,
    ) -> Result<RetirementReceipt, ClientError> {
        crate::authorized::validate_project_report(ds_project)?;
        self.ensure_active(now)?;
        crate::authorized::transformer_retirement(
            self.profile,
            transport,
            self.authority(),
            ds_project,
            action,
            request,
        )
    }

    /// Read one deleted transformer's verified delete backup, fetching its
    /// exact bytes when `fetch` is set.
    pub fn deleted_transformer_backup<T: AuthorizedTransport>(
        &self,
        transport: &mut T,
        ds_project: &str,
        request: &crate::DeletedBackupRequest,
        fetch: bool,
        now: u64,
    ) -> Result<crate::DeletedBackupRead, ClientError> {
        crate::authorized::validate_project_report(ds_project)?;
        self.ensure_active(now)?;
        crate::project_report::deleted_backup_read(
            self.profile,
            transport,
            self.authority(),
            ds_project,
            request,
            fetch,
        )
    }

    /// Sign the listed artifacts through the authorized signer and hand each
    /// fetched object's exact bytes and SHA-256 to `sink`, one at a time.
    pub fn download_artifacts<T: AuthorizedTransport>(
        &self,
        transport: &mut T,
        ds_project: &str,
        request: &crate::ArtifactDownloadRequest,
        sink: &mut dyn FnMut(crate::ArtifactFile) -> Result<(), ClientError>,
        now: u64,
    ) -> Result<Vec<(crate::ArtifactIdentity, crate::ArtifactOutcome)>, ClientError> {
        crate::authorized::validate_project_report(ds_project)?;
        self.ensure_active(now)?;
        crate::project_report::download_artifacts(
            self.profile,
            transport,
            self.authority(),
            ds_project,
            request,
            sink,
        )
    }

    /// Create a deleted transformer again from one pinned, verified backup.
    pub fn restore_deleted_transformer<T: AuthorizedTransport>(
        &self,
        transport: &mut T,
        ds_project: &str,
        request: &crate::DeletedBackupRequest,
        now: u64,
    ) -> Result<crate::DeletedTransformerRestoration, ClientError> {
        crate::authorized::validate_project_report(ds_project)?;
        self.ensure_active(now)?;
        crate::project_report::deleted_backup_restore(
            self.profile,
            transport,
            self.authority(),
            ds_project,
            request,
        )
    }
}

pub struct DeviceProtectedCall<'a> {
    session: &'a DeviceAccessSession,
    operation: DeviceProtectedOperation<'a>,
}

impl DeviceProtectedCall<'_> {
    pub fn method(&self) -> &'static str {
        match self.operation {
            DeviceProtectedOperation::List | DeviceProtectedOperation::Read(_) => "GET",
            DeviceProtectedOperation::Revoke(_) => "DELETE",
        }
    }
    pub fn path(&self) -> Result<String, DeviceError> {
        match self.operation {
            DeviceProtectedOperation::List => Ok("/api/v1/auth/devices".to_owned()),
            DeviceProtectedOperation::Read(id) | DeviceProtectedOperation::Revoke(id) => {
                bounded(id, 128)?;
                if !id
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
                {
                    return Err(DeviceError::Input);
                }
                Ok(format!("/api/v1/auth/devices/{id}"))
            }
        }
    }
    pub fn bearer_token(&self) -> &str {
        &self.session.access_token
    }
}

pub trait DeviceTransport {
    fn begin(&mut self, body: SecretRequestBody) -> Result<TransportResponse, TransportError>;
    fn status(&mut self, body: SecretRequestBody) -> Result<TransportResponse, TransportError>;
    fn complete(&mut self, body: SecretRequestBody) -> Result<TransportResponse, TransportError>;
    fn refresh(&mut self, body: SecretRequestBody) -> Result<TransportResponse, TransportError>;
    fn protected(
        &mut self,
        call: DeviceProtectedCall<'_>,
    ) -> Result<TransportResponse, TransportError>;
}

pub trait DeviceCredentialStore {
    fn acquire(&mut self, key: &str) -> Result<(), StoreError>;
    fn load(&mut self, key: &str) -> Result<Option<Vec<u8>>, StoreError>;
    fn compare_and_swap(
        &mut self,
        key: &str,
        expected: Option<&[u8]>,
        replacement: Option<&[u8]>,
    ) -> Result<(), StoreError>;
    fn release(&mut self, key: &str) -> Result<(), StoreError>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceError {
    Input,
    Binding,
    AccessToken,
    Response,
    AuthorityMismatch,
}

/// Why a device-authorization call made over a weak link has no answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeviceCallFailure<E = std::convert::Infallible> {
    /// The host could not supply an attempt's fresh proof (its clock or its
    /// RNG); nothing was sent for that attempt, and nothing is retried.
    Host(E),
    /// The request or its answer failed the closed device contract, or the
    /// endpoint refused it with a definitive answer (a 401, 403, 404, 409 or
    /// 410). Never retried.
    Device(DeviceError),
    /// Every attempt blinked: the last one timed out or lost the endpoint
    /// (`transport`), or answered a not-now status (`http_status`), after
    /// `retries` retries.
    Transient {
        transport: Option<TransportError>,
        http_status: Option<u16>,
        retries: u8,
    },
}

/// Send one device-authorization request, built afresh by `attempt` each
/// time, until an answer that is not a blink or the schedule is spent.
fn over_weak_network<E>(
    weak_network: &WeakNetwork,
    attempt: impl FnMut() -> Result<Result<TransportResponse, TransportError>, DeviceCallFailure<E>>,
) -> Result<TransportResponse, DeviceCallFailure<E>> {
    let (outcome, retries) = weak_network.run(attempt, |outcome| match outcome {
        Ok(Err(_)) => true,
        Ok(Ok(response)) => transient_status(response.status),
        Err(_) => false,
    });
    match outcome? {
        Err(transport) => Err(DeviceCallFailure::Transient {
            transport: Some(transport),
            http_status: None,
            retries,
        }),
        Ok(response) if transient_status(response.status) => Err(DeviceCallFailure::Transient {
            transport: None,
            http_status: Some(response.status),
            retries,
        }),
        Ok(response) => Ok(response),
    }
}

fn decode_success<T: for<'de> Deserialize<'de>>(
    response: TransportResponse,
) -> Result<StandardEnvelope<T>, DeviceError> {
    decode_status(response, 200)
}

fn decode_status<T: for<'de> Deserialize<'de>>(
    response: TransportResponse,
    expected_status: u16,
) -> Result<StandardEnvelope<T>, DeviceError> {
    if response.status != expected_status || response.body.len() > DEVICE_ACCESS_RESPONSE_LIMIT {
        return Err(DeviceError::Response);
    }
    serde_json::from_slice(&response.body).map_err(|_| DeviceError::Response)
}

fn validate_access_wire(
    token_type: &str,
    expires_in: u64,
    expires_at: &str,
) -> Result<(), DeviceError> {
    if token_type != "Bearer" || expires_in == 0 || expires_in > DEVICE_ACCESS_TTL_MAX_SECONDS {
        return Err(DeviceError::AccessToken);
    }
    bounded(expires_at, 64)
}

fn validate_summary(value: &DeviceSummary) -> Result<(), DeviceError> {
    bounded(&value.device_id, 128)?;
    bounded(&value.name, 100)?;
    if !matches!(value.platform.as_str(), "windows" | "linux" | "macos")
        || !matches!(value.status.as_str(), "active" | "expired" | "revoked")
        || value.scopes.as_slice() != [DEVICE_SCOPE]
    {
        return Err(DeviceError::Response);
    }
    digest_value(&value.fingerprint)?;
    validate_binding(&value.binding)?;
    for principal in [&value.principal, &value.approved_by] {
        bounded(&principal.uid, 256)?;
        bounded(&principal.email, 320)?;
    }
    for stamp in [
        &value.approved_at,
        &value.completed_at,
        &value.created_at,
        &value.last_used_at,
        &value.credential_expires_at,
    ] {
        bounded(stamp, 64)?;
    }
    if let Some(stamp) = &value.last_refreshed_at {
        bounded(stamp, 64)?;
    }
    if let Some(stamp) = &value.revoked_at {
        bounded(stamp, 64)?;
    }
    Ok(())
}

pub fn secret_json<T: Serialize>(value: &T) -> Result<SecretRequestBody, DeviceError> {
    Ok(SecretRequestBody::new(
        serde_json::to_vec(value).map_err(|_| DeviceError::Input)?,
    ))
}

fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
pub(crate) fn bounded(value: &str, max: usize) -> Result<(), DeviceError> {
    if value.is_empty()
        || value.len() > max
        || value.trim() != value
        || value.bytes().any(|b| b.is_ascii_control())
    {
        Err(DeviceError::Input)
    } else {
        Ok(())
    }
}
fn validate_verifier(value: &str) -> Result<(), DeviceError> {
    if (43..=128).contains(&value.len())
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~'))
    {
        Ok(())
    } else {
        Err(DeviceError::Input)
    }
}
fn validate_nonce(value: &str) -> Result<(), DeviceError> {
    let bytes = URL_SAFE_NO_PAD
        .decode(value)
        .map_err(|_| DeviceError::Input)?;
    if (16..=32).contains(&bytes.len()) {
        Ok(())
    } else {
        Err(DeviceError::Input)
    }
}
fn validate_public_key(value: &str) -> Result<(), DeviceError> {
    let bytes = URL_SAFE_NO_PAD
        .decode(value)
        .map_err(|_| DeviceError::Input)?;
    if bytes.len() == 32 {
        Ok(())
    } else {
        Err(DeviceError::Input)
    }
}
fn prefixed_digest(value: &str) -> Result<String, DeviceError> {
    let raw = value.strip_prefix("sha256:").unwrap_or(value);
    if raw.len() == 64
        && raw
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        Ok(format!("sha256:{raw}"))
    } else {
        Err(DeviceError::Binding)
    }
}
fn exact_prefixed_digest(value: &str) -> Result<(), ()> {
    let Some(raw) = value.strip_prefix("sha256:") else {
        return Err(());
    };
    if raw.len() == 64
        && raw
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        Ok(())
    } else {
        Err(())
    }
}
pub(crate) fn digest_value(value: &str) -> Result<(), DeviceError> {
    exact_prefixed_digest(value).map_err(|_| DeviceError::Response)
}
fn validate_begin_result(
    result: &DeviceBeginResult,
    expected_binding: &DeviceBinding,
) -> Result<(), DeviceError> {
    bounded(&result.request_id, 128)?;
    bounded(&result.device_code.0, 512)?;
    bounded(&result.user_code, 64)?;
    bounded(&result.expires_at, 64)?;
    bounded(&result.verification_uri, 512)?;
    digest_value(&result.device_fingerprint)?;
    if result.scopes.as_slice() != [DEVICE_SCOPE]
        || result.binding != *expected_binding
        || !(1..=60).contains(&result.poll_interval_seconds)
    {
        return Err(DeviceError::Response);
    }
    let uri = url::Url::parse(&result.verification_uri).map_err(|_| DeviceError::Response)?;
    if uri.scheme() != "https"
        || uri.host_str().is_none()
        || !uri.username().is_empty()
        || uri.password().is_some()
    {
        return Err(DeviceError::Response);
    }
    Ok(())
}
fn decode_private_key(value: &mut SecretString) -> Result<DevicePrivateKey, DeviceError> {
    let mut bytes = URL_SAFE_NO_PAD
        .decode(&value.0)
        .map_err(|_| DeviceError::Input)?;
    if bytes.len() != 32 {
        bytes.zeroize();
        return Err(DeviceError::Input);
    }
    let mut array = [0_u8; 32];
    array.copy_from_slice(&bytes);
    bytes.zeroize();
    value.0.zeroize();
    let key = DevicePrivateKey::from_secret_bytes(array);
    array.zeroize();
    Ok(key)
}
pub(crate) fn validate_binding(value: &DeviceBinding) -> Result<(), DeviceError> {
    if !matches!(value.lane.as_str(), "stable" | "canary")
        || value.audience.len() != 64
        || !value
            .audience
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(DeviceError::Binding);
    }
    for digest in [&value.profile_digest, &value.catalog_digest] {
        exact_prefixed_digest(digest).map_err(|_| DeviceError::Binding)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signature, Verifier};

    fn binding() -> DeviceBinding {
        DeviceBinding {
            lane: "stable".to_owned(),
            audience: "d".repeat(64),
            profile_digest: format!("sha256:{}", "a".repeat(64)),
            catalog_digest: format!("sha256:{}", "b".repeat(64)),
        }
    }

    #[test]
    fn complete_and_refresh_sign_only_the_frozen_transcripts() {
        let key = DevicePrivateKey::from_secret_bytes([7; 32]);
        let nonce = URL_SAFE_NO_PAD.encode([3; 16]);
        let verifier = "A".repeat(43);
        let complete = key
            .complete_signature("request-1", "device-secret", &verifier, &nonce)
            .unwrap();
        let complete_bytes = URL_SAFE_NO_PAD.decode(complete).unwrap();
        let complete_signature = Signature::from_slice(&complete_bytes).unwrap();
        let transcript = format!(
            "DS-DEVICE-COMPLETE-V1\nrequest-1\n{}\n{}\n{}",
            sha256_hex(b"device-secret"),
            sha256_hex(verifier.as_bytes()),
            nonce,
        );
        key.0
            .verifying_key()
            .verify(transcript.as_bytes(), &complete_signature)
            .unwrap();

        let refresh = key
            .refresh_signature("device-1", 1_900_000_000, &nonce, &binding())
            .unwrap();
        let refresh_signature =
            Signature::from_slice(&URL_SAFE_NO_PAD.decode(refresh).unwrap()).unwrap();
        let refresh_transcript = format!(
            "DS-DEVICE-REFRESH-V1\ndevice-1\n1900000000\n{}\nstable\n{}\nsha256:{}\nsha256:{}",
            nonce,
            "d".repeat(64),
            "a".repeat(64),
            "b".repeat(64),
        );
        key.0
            .verifying_key()
            .verify(refresh_transcript.as_bytes(), &refresh_signature)
            .unwrap();
    }

    #[test]
    fn begin_is_exact_pkce_and_contains_no_signature_or_secret_key() {
        let key = DevicePrivateKey::from_secret_bytes([9; 32]);
        let nonce = URL_SAFE_NO_PAD.encode([4; 16]);
        let verifier = "z".repeat(43);
        let request = DeviceBeginRequest::new(
            "Workstation",
            "linux",
            &key.public_key_base64url(),
            &nonce,
            &verifier,
            &binding(),
        )
        .unwrap();
        let body = secret_json(&request).unwrap();
        let json: serde_json::Value = serde_json::from_slice(body.as_bytes()).unwrap();
        assert_eq!(json["schema"], DEVICE_AUTH_SCHEMA);
        assert_eq!(json["public_key"]["alg"], "Ed25519");
        assert_eq!(json["public_key"]["encoding"], "base64url");
        assert_eq!(json["pkce"]["method"], "S256");
        assert_eq!(
            json["pkce"]["challenge"],
            URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
        );
        assert!(json.get("signature").is_none());
        assert!(
            !String::from_utf8_lossy(body.as_bytes()).contains(&URL_SAFE_NO_PAD.encode([9; 32]))
        );
    }

    #[test]
    fn memory_access_token_is_bounded_to_five_minutes_and_not_debuggable() {
        assert!(DeviceAccessSession::new("token".to_owned(), 1_601, 1_000).is_err());
        let session = DeviceAccessSession::new("token".to_owned(), 1_600, 1_000).unwrap();
        let call = session.protected_call(DeviceProtectedOperation::Read("device-1"));
        assert_eq!(call.method(), "GET");
        assert_eq!(call.path().unwrap(), "/api/v1/auth/devices/device-1");
        assert_eq!(call.bearer_token(), "token");
    }

    #[test]
    fn pending_bundle_exposes_only_the_public_begin_projection() {
        let nonce = URL_SAFE_NO_PAD.encode([6; 16]);
        let verifier = "v".repeat(43);
        let fingerprint = format!("sha256:{}", "e".repeat(64));
        let response = TransportResponse::new(
            201,
            serde_json::to_vec(&serde_json::json!({
                "success": true,
                "data": {
                    "request_id": "request-1",
                    "device_code": "device-code-secret",
                    "user_code": "ABCD-EFGH",
                    "verification_uri": "https://ds.example/account",
                    "expires_at": "2030-01-01T00:10:00Z",
                    "poll_interval_seconds": 3,
                    "device_fingerprint": fingerprint,
                    "scopes": ["ds.api"],
                    "binding": binding()
                }
            }))
            .unwrap(),
        );
        let begin = parse_device_begin(response, &binding()).unwrap();
        let pending = DevicePendingAuthorization::new(
            begin,
            verifier.clone(),
            nonce,
            DevicePrivateKey::from_secret_bytes([4; 32]),
        )
        .unwrap();
        let public = serde_json::to_string(&pending.public()).unwrap();
        assert!(!public.contains("device-code-secret"));
        assert!(!public.contains(&verifier));
        assert!(!public.contains(&URL_SAFE_NO_PAD.encode([4; 32])));
        let protected = pending.encode_protected().unwrap();
        assert!(String::from_utf8_lossy(protected.as_bytes()).contains("device-code-secret"));

        let mut pending = pending;
        pending.result.expires_at = "1970-01-01T01:00:10.5+01:00".to_owned();
        assert_eq!(pending.is_expired(10), Ok(false));
        assert_eq!(pending.is_expired(11), Ok(true));
        pending.result.expires_at = "1970-01-01T00:00:10Z".to_owned();
        assert_eq!(pending.is_expired(9), Ok(false));
        assert_eq!(pending.is_expired(10), Ok(true));
        pending.result.expires_at = "not a server deadline".to_owned();
        assert_eq!(pending.is_expired(u64::MAX), Err(DeviceError::Response));

        let wrong_status = TransportResponse::new(200, br#"{"data":{}}"#);
        assert!(parse_device_begin(wrong_status, &binding()).is_err());
    }

    #[test]
    fn server_digests_require_the_exact_wire_prefix() {
        let mut response_binding = binding();
        response_binding.profile_digest = "a".repeat(64);
        let response = TransportResponse::new(
            201,
            serde_json::to_vec(&serde_json::json!({
                "success": true,
                "data": {
                    "request_id": "request-1",
                    "device_code": "device-code-secret",
                    "user_code": "ABCD-EFGH",
                    "verification_uri": "https://ds.example/account",
                    "expires_at": "2030-01-01T00:10:00Z",
                    "poll_interval_seconds": 3,
                    "device_fingerprint": "e".repeat(64),
                    "scopes": ["ds.api"],
                    "binding": binding()
                }
            }))
            .unwrap(),
        );
        assert!(parse_device_begin(response, &binding()).is_err());
        assert!(validate_binding(&response_binding).is_err());
    }
}

#[cfg(test)]
mod weak_network_tests {
    use super::*;
    use std::collections::VecDeque;
    use std::time::Duration;

    const IMMEDIATE: WeakNetwork = WeakNetwork::new(&[Duration::ZERO; 3]);
    const NOW: u64 = 1_900_000_000;

    fn binding() -> DeviceBinding {
        DeviceBinding {
            lane: "stable".to_owned(),
            audience: "d".repeat(64),
            profile_digest: format!("sha256:{}", "a".repeat(64)),
            catalog_digest: format!("sha256:{}", "b".repeat(64)),
        }
    }

    /// Answers every refresh and status from a script, in order; an `Err` is
    /// a dropped connection. Begin and complete are never retried, so they
    /// are never reached.
    #[derive(Default)]
    struct Scripted {
        answers: VecDeque<Result<u16, TransportError>>,
        body: Vec<u8>,
        sent: Vec<serde_json::Value>,
    }

    impl Scripted {
        fn new(
            answers: impl IntoIterator<Item = Result<u16, TransportError>>,
            body: Value,
        ) -> Self {
            Self {
                answers: answers.into_iter().collect(),
                body: serde_json::to_vec(&body).unwrap(),
                sent: Vec::new(),
            }
        }
        fn answer(&mut self, body: SecretRequestBody) -> Result<TransportResponse, TransportError> {
            self.sent
                .push(serde_json::from_slice(body.as_bytes()).unwrap());
            let status = self.answers.pop_front().expect("unscripted device IO")?;
            Ok(TransportResponse::new(status, self.body.clone()))
        }
    }

    impl DeviceTransport for Scripted {
        fn begin(&mut self, _: SecretRequestBody) -> Result<TransportResponse, TransportError> {
            unreachable!("begin creates a request and is never retried")
        }
        fn status(&mut self, body: SecretRequestBody) -> Result<TransportResponse, TransportError> {
            self.answer(body)
        }
        fn complete(&mut self, _: SecretRequestBody) -> Result<TransportResponse, TransportError> {
            unreachable!("complete consumes a request and is never retried")
        }
        fn refresh(
            &mut self,
            body: SecretRequestBody,
        ) -> Result<TransportResponse, TransportError> {
            self.answer(body)
        }
        fn protected(
            &mut self,
            _: DeviceProtectedCall<'_>,
        ) -> Result<TransportResponse, TransportError> {
            unreachable!("protected calls are not part of sign-in")
        }
    }

    use serde_json::Value;

    fn credential() -> DeviceCredential {
        DeviceCredential::new(
            "device-1".to_owned(),
            "Workstation".to_owned(),
            "linux".to_owned(),
            format!("sha256:{}", "c".repeat(64)),
            "uid-1".to_owned(),
            "user@example.com".to_owned(),
            &binding(),
            "2031-01-01T00:00:00Z".to_owned(),
            DevicePrivateKey::from_secret_bytes([5; 32]),
        )
        .unwrap()
    }

    fn refreshed() -> Value {
        serde_json::json!({"data": {
            "access_token": "ds-access-jwt",
            "token_type": "Bearer",
            "expires_in": 300,
            "expires_at": "2030-03-17T17:51:40Z",
            "credential_expires_at": "2031-02-01T00:00:00Z"
        }})
    }

    /// A fresh `(timestamp, nonce)` per attempt, as a host's clock and RNG.
    fn proofs() -> impl FnMut() -> Result<(u64, String), &'static str> {
        let mut attempt = 0_u8;
        move || {
            attempt += 1;
            Ok((
                NOW + u64::from(attempt),
                URL_SAFE_NO_PAD.encode([attempt; 16]),
            ))
        }
    }

    #[test]
    fn a_refresh_survives_one_dropout_and_signs_the_retry_afresh() {
        let mut transport = Scripted::new([Err(TransportError::Unreachable), Ok(200)], refreshed());
        let (session, credential_expires_at) = credential()
            .refresh_session(&mut transport, &binding(), &IMMEDIATE, proofs())
            .unwrap();
        assert_eq!(session.expires_at(), NOW + 2 + 300);
        assert_eq!(credential_expires_at, "2031-02-01T00:00:00Z");
        // ds-brain spends a nonce the moment a proof arrives: the retry
        // carries its own nonce, timestamp and signature, never the first's.
        let [first, second] = transport.sent.as_slice() else {
            panic!("two attempts, {} sent", transport.sent.len());
        };
        for field in ["nonce", "timestamp", "signature"] {
            assert_ne!(first[field], second[field], "{field}");
        }
        assert_eq!(first["device_id"], second["device_id"]);
    }

    #[test]
    fn a_refresh_that_blinks_through_every_retry_is_transient_and_counts_them() {
        let mut transport = Scripted::new(
            [
                Err(TransportError::TimedOut),
                Ok(503),
                Err(TransportError::Unreachable),
                Ok(503),
            ],
            serde_json::json!({"error": {"code": "UNAVAILABLE"}}),
        );
        let failure = credential()
            .refresh_session(&mut transport, &binding(), &IMMEDIATE, proofs())
            .err()
            .expect("no session");
        assert_eq!(
            failure,
            DeviceCallFailure::Transient {
                transport: None,
                http_status: Some(503),
                retries: 3,
            }
        );
        assert_eq!(transport.sent.len(), 4);

        let mut transport = Scripted::new([Err(TransportError::TimedOut); 4], Value::Null);
        assert_eq!(
            credential()
                .refresh_session(&mut transport, &binding(), &IMMEDIATE, proofs())
                .err()
                .expect("no session"),
            DeviceCallFailure::Transient {
                transport: Some(TransportError::TimedOut),
                http_status: None,
                retries: 3,
            }
        );
    }

    #[test]
    fn a_definitive_refusal_is_answered_once_and_never_retried() {
        for status in [401, 403, 404, 409] {
            let mut transport = Scripted::new(
                [Ok(status), Ok(200)],
                serde_json::json!({"error": {"code": "DEVICE_CREDENTIAL_INVALID"}}),
            );
            let failure = credential()
                .refresh_session(&mut transport, &binding(), &IMMEDIATE, proofs())
                .err()
                .expect("no session");
            assert_eq!(failure, DeviceCallFailure::Device(DeviceError::Response));
            assert_eq!(transport.sent.len(), 1, "{status}");
        }
    }

    #[test]
    fn a_host_that_cannot_mint_a_proof_sends_nothing() {
        let mut transport = Scripted::new([Ok(200)], refreshed());
        let failure = credential()
            .refresh_session(&mut transport, &binding(), &IMMEDIATE, || {
                Err::<(u64, String), _>("rng unavailable")
            })
            .err()
            .expect("no session");
        assert_eq!(failure, DeviceCallFailure::Host("rng unavailable"));
        assert!(transport.sent.is_empty());
    }

    #[test]
    fn a_status_poll_survives_one_dropout_with_the_same_request() {
        let begin = parse_device_begin(
            TransportResponse::new(
                201,
                serde_json::to_vec(&serde_json::json!({"data": {
                    "request_id": "request-1",
                    "device_code": "device-code-secret",
                    "user_code": "ABCD-EFGH",
                    "verification_uri": "https://ds.example/account",
                    "expires_at": "2030-01-01T00:10:00Z",
                    "poll_interval_seconds": 3,
                    "device_fingerprint": format!("sha256:{}", "e".repeat(64)),
                    "scopes": ["ds.api"],
                    "binding": binding()
                }}))
                .unwrap(),
            ),
            &binding(),
        )
        .unwrap();
        let pending = DevicePendingAuthorization::new(
            begin,
            "v".repeat(43),
            URL_SAFE_NO_PAD.encode([6; 16]),
            DevicePrivateKey::from_secret_bytes([4; 32]),
        )
        .unwrap();
        let mut transport = Scripted::new(
            [Err(TransportError::TimedOut), Ok(200)],
            serde_json::json!({"data": {
                "status": "approved",
                "expires_at": "2030-01-01T00:10:00Z",
                "poll_interval_seconds": 3
            }}),
        );
        let status = pending.observe_status(&mut transport, &IMMEDIATE).unwrap();
        assert_eq!(status.status, DeviceAuthorizationStatus::Approved);
        assert_eq!(transport.sent.len(), 2);
        assert_eq!(transport.sent[0], transport.sent[1]);

        let mut transport = Scripted::new([Ok(503); 4], Value::Null);
        assert_eq!(
            pending
                .observe_status(&mut transport, &IMMEDIATE)
                .unwrap_err(),
            DeviceCallFailure::Transient {
                transport: None,
                http_status: Some(503),
                retries: 3,
            }
        );
        let mut transport = Scripted::new([Ok(404), Ok(200)], Value::Null);
        assert_eq!(
            pending
                .observe_status(&mut transport, &IMMEDIATE)
                .unwrap_err(),
            DeviceCallFailure::Device(DeviceError::Response)
        );
        assert_eq!(transport.sent.len(), 1);
    }
}
