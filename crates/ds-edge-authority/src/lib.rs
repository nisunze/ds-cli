//! Native verification for ds-brain desktop edge capabilities.
//!
//! This crate deliberately has no Tauri dependency. The desktop shell supplies
//! its private authority directory and shell-minted identity, while this module
//! owns signature verification, the bounded Google certificate cache, closed
//! capability schemas, and the rollback-resistant server-time high-water mark.

use base64::{engine::general_purpose, Engine as _};
use chrono::{DateTime, Duration as ChronoDuration, Utc};
use rsa::pkcs1v15::{Signature as RsaSignature, VerifyingKey};
use rsa::pkcs8::DecodePublicKey;
use rsa::signature::Verifier as _;
use rsa::traits::PublicKeyParts;
use rsa::RsaPublicKey;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use x509_cert::der::{Decode, Encode};
use x509_cert::Certificate;

mod install_identity;
mod jcs;
mod offline_install;
pub use install_identity::load_or_create_install_id;
pub use offline_install::RetainedDesktopLicense;

pub use jcs::{
    network_reporter_room_content_sha256, rfc8785_canonicalize, rfc8785_canonicalize_json,
    rfc8785_sha256,
};

const INSTALL_DOMAIN: &str = "ds-brain/install-lease/v1";
const WORK_DOMAIN: &str = "ds-brain/work-grant/v1";
const ARTIFACT_READ_DOMAIN: &str = "ds-brain/network-reporter-artifact-read/v1";
const CERT_ENDPOINT: &str = "https://www.googleapis.com/robot/v1/metadata/x509/";
const HIGH_WATER_FILE: &str = "server-time-high-water.v1.json";
const CERT_CACHE_FILE: &str = "signer-certificates.v1.json";
const AUTHORITY_LOCK_FILE: &str = ".edge-authority.lock";
const HIGH_WATER_CONTRACT: &str = "ds-edge-server-time-high-water/v1";
const CERT_CACHE_CONTRACT: &str = "ds-edge-signer-certificates/v1";
const MAX_CAPABILITY_BYTES: usize = 16 * 1024;
const MAX_SIGNATURE_BYTES: usize = 1024;
const MAX_CERT_RESPONSE_BYTES: u64 = 256 * 1024;
const MAX_CERTIFICATE_BYTES: usize = 16 * 1024;
const MAX_AUTHORITY_RECORD_BYTES: u64 = 320 * 1024;
const INSTALL_LEASE_MAX_SECONDS: i64 = ds_command_kernel::installation::OFFLINE_LEASE_SECONDS;
const WORK_GRANT_MAX_SECONDS: i64 = 24 * 60 * 60;
const ARTIFACT_READ_MAX_SECONDS: i64 = 5 * 60;
const HTTP_CONNECT_TIMEOUT: Duration = Duration::from_secs(3);
const HTTP_READ_TIMEOUT: Duration = Duration::from_secs(5);
const HTTP_OVERALL_TIMEOUT: Duration = Duration::from_secs(10);
const AUTHORITY_LOCK_ATTEMPTS: usize = 80;
const AUTHORITY_LOCK_RETRY: Duration = Duration::from_millis(25);

#[derive(Debug, Clone)]
pub struct AuthorityPins {
    pub lane: String,
    pub signer: String,
    pub issuer: String,
    pub audience: String,
}

impl AuthorityPins {
    pub fn new(
        lane: impl Into<String>,
        signer: impl Into<String>,
        issuer: impl Into<String>,
        audience: impl Into<String>,
    ) -> Result<Self, String> {
        let pins = Self {
            lane: lane.into(),
            signer: signer.into(),
            issuer: issuer.into(),
            audience: audience.into(),
        };
        if !matches!(pins.lane.as_str(), "canary" | "stable")
            || !valid_service_account(&pins.signer)
            || !valid_issuer(&pins.issuer)
            || pins.audience != "urn:rw.datasolutions:desktop-edge"
        {
            return Err("desktop edge authority build pins are invalid".to_string());
        }
        Ok(pins)
    }

    fn identity(&self) -> String {
        let mut digest = Sha256::new();
        for value in [&self.lane, &self.signer, &self.issuer, &self.audience] {
            digest.update(value.as_bytes());
            digest.update([0]);
        }
        format!("{:x}", digest.finalize())
    }
}

/// Bound the coarse native OS label used by the install registry. The caller
/// obtains these two values from an OS API/library; this helper deliberately
/// has no hardware or path input.
pub fn bounded_os_version(os_type: &str, version: &str) -> Result<String, String> {
    let os_type = os_type.trim();
    let version = version.trim();
    if os_type.is_empty()
        || version.is_empty()
        || os_type.eq_ignore_ascii_case("unknown")
        || version.eq_ignore_ascii_case("unknown")
    {
        return Err("desktop operating-system version is unavailable".to_string());
    }
    let value = format!("{os_type} {version}");
    if value.len() > 120 || value.chars().any(char::is_control) {
        return Err("desktop operating-system version is invalid".to_string());
    }
    Ok(value)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SignatureDeclaration {
    pub algorithm: String,
    pub signer: String,
    pub key_id: String,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Principal {
    pub uid: String,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_present_organization"
    )]
    pub org: Option<String>,
}

fn deserialize_present_organization<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    // `default` handles omission. If the key is present it must carry an exact
    // string; JSON null must not collapse into the anonymous/no-org shape.
    String::deserialize(deserializer).map(Some)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct InstallLeasePayload {
    schema: u8,
    issuer: String,
    audience: String,
    install_id: String,
    device: String,
    arch: String,
    app_version: String,
    channel: String,
    principal: Principal,
    status: String,
    min_supported_version: String,
    issued_at: String,
    expires_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InstallLeaseCapability {
    pub schema: u8,
    pub issuer: String,
    pub audience: String,
    pub install_id: String,
    pub device: String,
    pub arch: String,
    pub app_version: String,
    pub channel: String,
    pub principal: Principal,
    pub status: String,
    pub min_supported_version: String,
    pub issued_at: String,
    pub expires_at: String,
    pub signed_payload: String,
    pub signature: SignatureDeclaration,
}

impl InstallLeaseCapability {
    fn visible_payload(&self) -> InstallLeasePayload {
        InstallLeasePayload {
            schema: self.schema,
            issuer: self.issuer.clone(),
            audience: self.audience.clone(),
            install_id: self.install_id.clone(),
            device: self.device.clone(),
            arch: self.arch.clone(),
            app_version: self.app_version.clone(),
            channel: self.channel.clone(),
            principal: self.principal.clone(),
            status: self.status.clone(),
            min_supported_version: self.min_supported_version.clone(),
            issued_at: self.issued_at.clone(),
            expires_at: self.expires_at.clone(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExpectedInstall {
    pub install_id: String,
    pub device: String,
    pub arch: String,
    pub app_version: String,
    pub channel: String,
    /// None is an intentional pre-sign-in heartbeat. It must match a signed
    /// principal `{uid:""}` with no organization; it is never a wildcard.
    pub owner_uid: Option<String>,
    pub status: String,
    pub min_supported_version: String,
    pub lease_expires_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VerifyInstallRequest {
    pub server_time: String,
    pub capability: InstallLeaseCapability,
    pub expected: ExpectedInstall,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VerifiedInstallLease {
    pub status: VerifiedInstallStatus,
    pub owner_uid: Option<String>,
    pub organization_id: Option<String>,
    pub issued_at: String,
    pub lease_expires_at: String,
    pub server_time: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum VerifiedInstallStatus {
    Active,
    Blocked,
    UpgradeRequired,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct GrantEngine {
    name: String,
    release: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct GrantScope {
    ds_project: String,
    capabilities: Vec<String>,
    engines: Vec<GrantEngine>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct WorkGrantPayload {
    schema: u8,
    issuer: String,
    audience: String,
    grant_id: String,
    install_id: String,
    device: String,
    principal: Principal,
    scope: GrantScope,
    snapshot_base: BTreeMap<String, String>,
    individual_execution: IndividualExecution,
    issued_at: String,
    expires_at: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkGrantCapability {
    pub schema: u8,
    pub issuer: String,
    pub audience: String,
    pub grant_id: String,
    pub install_id: String,
    pub device: String,
    pub principal: Principal,
    pub scope: WorkGrantScope,
    pub snapshot_base: BTreeMap<String, String>,
    pub individual_execution: IndividualExecution,
    pub issued_at: String,
    pub expires_at: String,
    pub signed_payload: String,
    pub signature: SignatureDeclaration,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkGrantScope {
    pub ds_project: String,
    pub capabilities: Vec<String>,
    pub engines: Vec<WorkGrantEngine>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkGrantEngine {
    pub name: String,
    pub release: String,
}

impl WorkGrantCapability {
    fn visible_payload(&self) -> WorkGrantPayload {
        WorkGrantPayload {
            schema: self.schema,
            issuer: self.issuer.clone(),
            audience: self.audience.clone(),
            grant_id: self.grant_id.clone(),
            install_id: self.install_id.clone(),
            device: self.device.clone(),
            principal: self.principal.clone(),
            scope: GrantScope {
                ds_project: self.scope.ds_project.clone(),
                capabilities: self.scope.capabilities.clone(),
                engines: self
                    .scope
                    .engines
                    .iter()
                    .map(|engine| GrantEngine {
                        name: engine.name.clone(),
                        release: engine.release.clone(),
                    })
                    .collect(),
            },
            snapshot_base: self.snapshot_base.clone(),
            individual_execution: self.individual_execution.clone(),
            issued_at: self.issued_at.clone(),
            expires_at: self.expires_at.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ArtifactReadEntitlementPayload {
    schema: u8,
    issuer: String,
    audience: String,
    install_id: String,
    device: String,
    principal: Principal,
    scope: GrantScope,
    grant_id: String,
    batch_id: String,
    individual_execution: IndividualExecution,
    issued_at: String,
    expires_at: String,
}

/// One short-lived server assertion authorizing native access to an exact
/// crash-durable Network Reporter publication. It deliberately carries no
/// locator, filename, or filesystem path.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArtifactReadEntitlementCapability {
    pub schema: u8,
    pub issuer: String,
    pub audience: String,
    pub install_id: String,
    pub device: String,
    pub principal: Principal,
    pub scope: WorkGrantScope,
    pub grant_id: String,
    pub batch_id: String,
    pub individual_execution: IndividualExecution,
    pub issued_at: String,
    pub expires_at: String,
    pub signed_payload: String,
    pub signature: SignatureDeclaration,
}

impl ArtifactReadEntitlementCapability {
    fn visible_payload(&self) -> ArtifactReadEntitlementPayload {
        ArtifactReadEntitlementPayload {
            schema: self.schema,
            issuer: self.issuer.clone(),
            audience: self.audience.clone(),
            install_id: self.install_id.clone(),
            device: self.device.clone(),
            principal: self.principal.clone(),
            scope: GrantScope {
                ds_project: self.scope.ds_project.clone(),
                capabilities: self.scope.capabilities.clone(),
                engines: self
                    .scope
                    .engines
                    .iter()
                    .map(|engine| GrantEngine {
                        name: engine.name.clone(),
                        release: engine.release.clone(),
                    })
                    .collect(),
            },
            grant_id: self.grant_id.clone(),
            batch_id: self.batch_id.clone(),
            individual_execution: self.individual_execution.clone(),
            issued_at: self.issued_at.clone(),
            expires_at: self.expires_at.clone(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExpectedArtifactRead {
    pub install_id: String,
    pub device: String,
    pub owner_uid: String,
    pub organization_id: Option<String>,
    pub grant_id: String,
    pub batch_id: String,
    pub lease_expires_at: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VerifyArtifactReadRequest {
    pub server_time: String,
    pub capability: ArtifactReadEntitlementCapability,
    pub expected: ExpectedArtifactRead,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VerifiedArtifactReadEntitlement {
    pub owner_uid: String,
    pub organization_id: Option<String>,
    pub ds_project: String,
    pub engine_release: String,
    pub grant_id: String,
    pub batch_id: String,
    pub individual_execution: VerifiedIndividualExecution,
    pub issued_at: String,
    pub expires_at: String,
    pub server_time: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExpectedWork {
    pub install_id: String,
    pub device: String,
    pub owner_uid: String,
    pub organization_id: Option<String>,
    pub ds_project: String,
    pub engine_release: String,
    pub transformer: String,
    pub lease_expires_at: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VerifyWorkRequest {
    pub server_time: String,
    pub capability: WorkGrantCapability,
    pub snapshot_manifest: NetworkReporterSnapshotManifest,
    pub expected: ExpectedWork,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NetworkReporterSnapshotManifest {
    pub schema: u8,
    pub engine: SnapshotEngine,
    pub transformers: Vec<SnapshotTransformer>,
    pub individual_execution: IndividualExecution,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SnapshotEngine {
    pub name: String,
    pub release: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SnapshotTransformer {
    pub name: String,
    pub revision: i64,
    pub input_base_fingerprint: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IndividualExecution {
    pub transformer: String,
    pub revision: i64,
    pub input_base_fingerprint: String,
    pub room_content_sha256: String,
    pub outputs: Vec<IndividualExecutionOutput>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IndividualExecutionOutput {
    pub output_id: String,
    pub format: String,
    pub content_type: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VerifiedIndividualExecution {
    pub transformer: String,
    pub revision: i64,
    pub input_base_fingerprint: String,
    pub room_content_sha256: String,
    pub outputs: Vec<VerifiedIndividualExecutionOutput>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VerifiedIndividualExecutionOutput {
    pub output_id: String,
    pub format: String,
    pub content_type: String,
}

impl From<IndividualExecution> for VerifiedIndividualExecution {
    fn from(value: IndividualExecution) -> Self {
        Self {
            transformer: value.transformer,
            revision: value.revision,
            input_base_fingerprint: value.input_base_fingerprint,
            room_content_sha256: value.room_content_sha256,
            outputs: value
                .outputs
                .into_iter()
                .map(|output| VerifiedIndividualExecutionOutput {
                    output_id: output.output_id,
                    format: output.format,
                    content_type: output.content_type,
                })
                .collect(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VerifiedWorkGrant {
    pub grant_id: String,
    pub engine_release: String,
    pub snapshot_base_fingerprint: String,
    pub transformer_revision: i64,
    pub individual_execution: VerifiedIndividualExecution,
    pub issued_at: String,
    pub expires_at: String,
    pub server_time: String,
}

#[derive(Debug, Clone)]
pub struct AuthorityVerifier {
    authority_dir: PathBuf,
    pins: AuthorityPins,
}

impl AuthorityVerifier {
    pub fn new(authority_dir: impl Into<PathBuf>, pins: AuthorityPins) -> Result<Self, String> {
        let authority_dir = authority_dir.into();
        validate_authority_directory(&authority_dir)?;
        Ok(Self {
            authority_dir,
            pins,
        })
    }

    pub fn verify_install(
        &self,
        request: VerifyInstallRequest,
    ) -> Result<VerifiedInstallLease, String> {
        let verified = self.verify_install_with_source(
            request.clone(),
            &GoogleCertificateSource,
            unix_now()?,
        )?;
        self.retain_install(request)?;
        Ok(verified)
    }

    pub fn verify_work(&self, request: VerifyWorkRequest) -> Result<VerifiedWorkGrant, String> {
        self.verify_work_with_source(request, &GoogleCertificateSource, unix_now()?)
    }

    pub fn verify_artifact_read(
        &self,
        request: VerifyArtifactReadRequest,
    ) -> Result<VerifiedArtifactReadEntitlement, String> {
        self.verify_artifact_read_with_source(request, &GoogleCertificateSource, unix_now()?)
    }

    fn verify_install_with_source(
        &self,
        request: VerifyInstallRequest,
        source: &dyn CertificateSource,
        local_now: i64,
    ) -> Result<VerifiedInstallLease, String> {
        let keys = CachedGoogleKeySource {
            verifier: self,
            source,
            local_now,
        };
        self.verify_install_with_keys(request, &keys)
    }

    fn verify_install_with_keys(
        &self,
        request: VerifyInstallRequest,
        keys: &dyn PublicKeySource,
    ) -> Result<VerifiedInstallLease, String> {
        self.validate_install_with_keys(request, keys, true)
    }

    fn validate_install_with_keys(
        &self,
        request: VerifyInstallRequest,
        keys: &dyn PublicKeySource,
        observe: bool,
    ) -> Result<VerifiedInstallLease, String> {
        let decoded: InstallLeasePayload = self.verify_signed_payload(
            INSTALL_DOMAIN,
            &request.capability.signed_payload,
            &request.capability.signature,
            keys,
        )?;
        let visible = request.capability.visible_payload();
        if decoded != visible {
            return Err("signed install lease does not match its visible capability".to_string());
        }
        let expected = request.expected;
        let principal_matches = match expected.owner_uid.as_deref() {
            Some(owner_uid) => bounded_text(owner_uid, 128) && decoded.principal.uid == owner_uid,
            None => decoded.principal.uid.is_empty() && decoded.principal.org.is_none(),
        };
        if decoded.schema != 1
            || decoded.issuer != self.pins.issuer
            || decoded.audience != self.pins.audience
            || decoded.install_id != expected.install_id
            || decoded.device != expected.device
            || decoded.arch != expected.arch
            || decoded.app_version != expected.app_version
            || decoded.channel != expected.channel
            || !principal_matches
            || decoded.status != expected.status
            || !matches!(decoded.status.as_str(), "active" | "blocked")
            || decoded.min_supported_version != expected.min_supported_version
            || decoded.expires_at != expected.lease_expires_at
            || !canonical_uuid_v4(&decoded.install_id)
            || !matches!(decoded.device.as_str(), "windows" | "linux" | "macos")
            || decoded.arch != "x86_64"
            || !matches!(decoded.channel.as_str(), "canary" | "stable")
            || (!decoded.principal.uid.is_empty() && !bounded_text(&decoded.principal.uid, 128))
            || decoded
                .principal
                .org
                .as_ref()
                .is_some_and(|value| !bounded_text(value, 128))
            || !bounded_text(&decoded.min_supported_version, 64)
        {
            return Err(
                "signed install lease does not match this desktop installation".to_string(),
            );
        }
        let server_time = parse_timestamp(&request.server_time, "heartbeat server_time")?;
        let issued_at = parse_timestamp(&decoded.issued_at, "install lease issued_at")?;
        let expires_at = parse_timestamp(&decoded.expires_at, "install lease expires_at")?;
        validate_window(
            issued_at,
            expires_at,
            server_time,
            INSTALL_LEASE_MAX_SECONDS,
            "install lease",
        )?;
        let app_version = semver::Version::parse(&decoded.app_version).map_err(|_| {
            "signed install lease app version is not semantic version text".to_string()
        })?;
        let minimum = semver::Version::parse(&decoded.min_supported_version).map_err(|_| {
            "signed install lease minimum version is not semantic version text".to_string()
        })?;
        if app_version.to_string() != decoded.app_version
            || minimum.to_string() != decoded.min_supported_version
        {
            return Err(
                "signed install lease version text is not canonical semantic version text"
                    .to_string(),
            );
        }
        let status = if decoded.status == "blocked" {
            VerifiedInstallStatus::Blocked
        } else if app_version < minimum {
            VerifiedInstallStatus::UpgradeRequired
        } else {
            VerifiedInstallStatus::Active
        };
        if observe {
            self.observe_server_time(&request.server_time, server_time)?;
        }
        Ok(VerifiedInstallLease {
            status,
            owner_uid: (!decoded.principal.uid.is_empty()).then_some(decoded.principal.uid),
            organization_id: decoded.principal.org,
            issued_at: decoded.issued_at,
            lease_expires_at: decoded.expires_at,
            server_time: request.server_time,
        })
    }

    fn verify_work_with_source(
        &self,
        request: VerifyWorkRequest,
        source: &dyn CertificateSource,
        local_now: i64,
    ) -> Result<VerifiedWorkGrant, String> {
        let keys = CachedGoogleKeySource {
            verifier: self,
            source,
            local_now,
        };
        self.verify_work_with_keys(request, &keys)
    }

    fn verify_work_with_keys(
        &self,
        request: VerifyWorkRequest,
        keys: &dyn PublicKeySource,
    ) -> Result<VerifiedWorkGrant, String> {
        let decoded: WorkGrantPayload = self.verify_signed_payload(
            WORK_DOMAIN,
            &request.capability.signed_payload,
            &request.capability.signature,
            keys,
        )?;
        let visible = request.capability.visible_payload();
        if decoded != visible {
            return Err("signed work grant does not match its visible capability".to_string());
        }
        let expected = request.expected;
        let operation = format!("export-{}", expected.transformer);
        let fingerprint = decoded.snapshot_base.get(&operation).cloned();
        let manifest = request.snapshot_manifest;
        let manifest_transformer = manifest.transformers.first();
        let execution = &decoded.individual_execution;
        if decoded.schema != 1
            || decoded.issuer != self.pins.issuer
            || decoded.audience != self.pins.audience
            || decoded.install_id != expected.install_id
            || decoded.device != expected.device
            || decoded.principal.uid != expected.owner_uid
            || decoded.principal.org != expected.organization_id
            || decoded.scope.ds_project != expected.ds_project
            || decoded.scope.capabilities != ["design.report"]
            || decoded.scope.engines
                != [GrantEngine {
                    name: "ds-network-reporter".to_string(),
                    release: expected.engine_release.clone(),
                }]
            || decoded.snapshot_base.len() != 1
            || fingerprint
                .as_ref()
                .is_none_or(|value| !valid_sha256(value))
            || !valid_edge_id(&decoded.grant_id)
            || !canonical_uuid_v4(&decoded.install_id)
            || !matches!(decoded.device.as_str(), "windows" | "linux" | "macos")
            || !bounded_text(&decoded.principal.uid, 128)
            || decoded
                .principal
                .org
                .as_ref()
                .is_some_and(|value| !bounded_text(value, 128))
            || !valid_edge_id(&decoded.scope.ds_project)
            || !valid_transformer(&expected.transformer)
            || !valid_reporter_release(&expected.engine_release)
            || manifest.schema != 1
            || manifest.engine.name != "ds-network-reporter"
            || manifest.engine.release != expected.engine_release
            || manifest.transformers.len() != 1
            || manifest.individual_execution != decoded.individual_execution
            || manifest_transformer.is_none_or(|transformer| {
                transformer.name != expected.transformer
                    || !(1..=9_007_199_254_740_991).contains(&transformer.revision)
                    || Some(&transformer.input_base_fingerprint) != fingerprint.as_ref()
            })
            || execution.transformer != expected.transformer
            || !(1..=9_007_199_254_740_991).contains(&execution.revision)
            || execution.input_base_fingerprint != fingerprint.as_deref().unwrap_or_default()
            || !valid_sha256(&execution.room_content_sha256)
            || !valid_individual_outputs(&execution.outputs)
            || manifest_transformer.is_none_or(|transformer| {
                transformer.name != execution.transformer
                    || transformer.revision != execution.revision
                    || transformer.input_base_fingerprint != execution.input_base_fingerprint
            })
        {
            return Err("signed work grant does not match the requested desktop work".to_string());
        }
        let server_time = parse_timestamp(&request.server_time, "work grant server_time")?;
        let issued_at = parse_timestamp(&decoded.issued_at, "work grant issued_at")?;
        let expires_at = parse_timestamp(&decoded.expires_at, "work grant expires_at")?;
        let lease_expires_at = parse_timestamp(
            &expected.lease_expires_at,
            "verified install lease expires_at",
        )?;
        validate_window(
            issued_at,
            expires_at,
            server_time,
            WORK_GRANT_MAX_SECONDS,
            "work grant",
        )?;
        if expires_at > lease_expires_at {
            return Err("signed work grant outlives its verified install lease".to_string());
        }
        self.observe_server_time(&request.server_time, server_time)?;
        let individual_execution = decoded.individual_execution.into();
        Ok(VerifiedWorkGrant {
            grant_id: decoded.grant_id,
            engine_release: expected.engine_release,
            snapshot_base_fingerprint: fingerprint.expect("validated fingerprint exists"),
            transformer_revision: manifest_transformer
                .expect("validated transformer manifest exists")
                .revision,
            individual_execution,
            issued_at: decoded.issued_at,
            expires_at: decoded.expires_at,
            server_time: request.server_time,
        })
    }

    fn verify_artifact_read_with_source(
        &self,
        request: VerifyArtifactReadRequest,
        source: &dyn CertificateSource,
        local_now: i64,
    ) -> Result<VerifiedArtifactReadEntitlement, String> {
        let keys = CachedGoogleKeySource {
            verifier: self,
            source,
            local_now,
        };
        self.verify_artifact_read_with_keys(request, &keys)
    }

    fn verify_artifact_read_with_keys(
        &self,
        request: VerifyArtifactReadRequest,
        keys: &dyn PublicKeySource,
    ) -> Result<VerifiedArtifactReadEntitlement, String> {
        let decoded: ArtifactReadEntitlementPayload = self.verify_signed_payload(
            ARTIFACT_READ_DOMAIN,
            &request.capability.signed_payload,
            &request.capability.signature,
            keys,
        )?;
        if decoded != request.capability.visible_payload() {
            return Err(
                "signed Network Reporter artifact-read entitlement does not match its visible capability"
                    .to_string(),
            );
        }
        let expected = request.expected;
        let execution = &decoded.individual_execution;
        let signed_engine_release = decoded
            .scope
            .engines
            .first()
            .map(|engine| engine.release.clone());
        if decoded.schema != 1
            || decoded.issuer != self.pins.issuer
            || decoded.audience != self.pins.audience
            || decoded.install_id != expected.install_id
            || decoded.device != expected.device
            || decoded.principal.uid != expected.owner_uid
            || decoded.principal.org != expected.organization_id
            || decoded.scope.capabilities != ["design.report"]
            || decoded.scope.engines.len() != 1
            || decoded.scope.engines.first().is_none_or(|engine| {
                engine.name != "ds-network-reporter" || !valid_reporter_release(&engine.release)
            })
            || decoded.grant_id != expected.grant_id
            || decoded.batch_id != expected.batch_id
            || !canonical_uuid_v4(&decoded.install_id)
            || !matches!(decoded.device.as_str(), "windows" | "linux" | "macos")
            || !bounded_text(&decoded.principal.uid, 128)
            || decoded
                .principal
                .org
                .as_ref()
                .is_some_and(|value| !bounded_text(value, 128))
            || !valid_edge_id(&decoded.scope.ds_project)
            || !valid_network_reporter_grant_id(&decoded.grant_id)
            || !valid_sha256(&decoded.batch_id)
            || !valid_transformer(&execution.transformer)
            || !(1..=9_007_199_254_740_991).contains(&execution.revision)
            || !valid_sha256(&execution.input_base_fingerprint)
            || !valid_sha256(&execution.room_content_sha256)
            || !valid_individual_outputs(&execution.outputs)
        {
            return Err(
                "signed Network Reporter artifact-read entitlement does not match this desktop publication"
                    .to_string(),
            );
        }
        let server_time = parse_timestamp(
            &request.server_time,
            "Network Reporter artifact-read server_time",
        )?;
        let issued_at = parse_timestamp(
            &decoded.issued_at,
            "Network Reporter artifact-read issued_at",
        )?;
        let expires_at = parse_timestamp(
            &decoded.expires_at,
            "Network Reporter artifact-read expires_at",
        )?;
        let lease_expires_at = parse_timestamp(
            &expected.lease_expires_at,
            "verified install lease expires_at",
        )?;
        validate_window(
            issued_at,
            expires_at,
            server_time,
            ARTIFACT_READ_MAX_SECONDS,
            "Network Reporter artifact-read entitlement",
        )?;
        if expires_at > lease_expires_at {
            return Err(
                "signed Network Reporter artifact-read entitlement outlives its verified install lease"
                    .to_string(),
            );
        }
        self.observe_server_time(&request.server_time, server_time)?;
        Ok(VerifiedArtifactReadEntitlement {
            owner_uid: decoded.principal.uid,
            organization_id: decoded.principal.org,
            ds_project: decoded.scope.ds_project,
            engine_release: signed_engine_release.expect("validated signed engine exists"),
            grant_id: decoded.grant_id,
            batch_id: decoded.batch_id,
            individual_execution: decoded.individual_execution.into(),
            issued_at: decoded.issued_at,
            expires_at: decoded.expires_at,
            server_time: request.server_time,
        })
    }

    fn verify_signed_payload<T: for<'de> Deserialize<'de>>(
        &self,
        domain: &str,
        encoded_payload: &str,
        signature: &SignatureDeclaration,
        keys: &dyn PublicKeySource,
    ) -> Result<T, String> {
        if signature.algorithm != "RS256"
            || signature.signer != self.pins.signer
            || !valid_key_id(&signature.key_id)
        {
            return Err("signed capability has an invalid or unpinned signer".to_string());
        }
        let payload =
            decode_raw_base64url(encoded_payload, MAX_CAPABILITY_BYTES, "signed payload")?;
        let signature_bytes =
            decode_raw_base64url(&signature.value, MAX_SIGNATURE_BYTES, "signature")?;
        let prefix = format!("{domain}\0");
        if !payload.starts_with(prefix.as_bytes()) || payload.len() == prefix.len() {
            return Err("signed capability has the wrong domain separation".to_string());
        }
        let key = keys.resolve(&signature.key_id)?;
        let parsed_signature = RsaSignature::try_from(signature_bytes.as_slice())
            .map_err(|_| "signed capability has an invalid RSA signature shape".to_string())?;
        VerifyingKey::<Sha256>::new(key)
            .verify(&payload, &parsed_signature)
            .map_err(|_| "signed capability signature did not verify".to_string())?;
        serde_json::from_slice(&payload[prefix.len()..])
            .map_err(|error| format!("signed capability JSON is invalid or not closed: {error}"))
    }

    /// The signing key for `key_id`, from the retained certificate set when it
    /// still holds one, otherwise from one bounded fetch.
    ///
    /// A cached certificate is served for the window its ISSUER declared —
    /// the certificate's own `notBefore`/`notAfter` — not for an hour after
    /// this machine happened to fetch it. Reaching the endpoint is therefore
    /// never a precondition for verifying with a certificate that is still
    /// valid: a desktop behind a blocked or slow network keeps working, which
    /// is the whole point of retaining the set. The endpoint is consulted only
    /// when nothing usable is held for this key — it was never cached, it was
    /// rotated away, or its own validity ran out — so Google-managed key
    /// rotation still forces exactly one bounded refresh.
    fn resolve_public_key(
        &self,
        key_id: &str,
        source: &dyn CertificateSource,
        local_now: i64,
    ) -> Result<RsaPublicKey, String> {
        let cache_path = self.authority_dir.join(CERT_CACHE_FILE);
        // Why the cached certificate cannot be used, when one is held for this
        // key at all. It names the certificate's own expiry, so an offline
        // failure says which of the two facts refused the call.
        let mut cached_expiry: Option<i64> = None;
        if let Ok(Some(raw)) = read_bounded_file(&cache_path, MAX_AUTHORITY_RECORD_BYTES) {
            if let Ok(cache) = serde_json::from_slice::<CertificateCache>(&raw) {
                if validate_certificate_cache(&cache, &self.pins, local_now).is_ok() {
                    if let Some(encoded) = cache.certificates.get(key_id) {
                        if let Ok(certificate) = parse_certificate(encoded) {
                            if certificate.covers(local_now) {
                                return Ok(certificate.key);
                            }
                            cached_expiry = Some(certificate.not_after_unix);
                        }
                    }
                }
            }
        }

        // Nothing usable is retained for this key, so the endpoint is the only
        // remaining source. It never falls back to a response-provided one.
        let certificates = source.fetch(&self.pins.signer).map_err(|error| match cached_expiry {
            Some(expiry) => format!(
                "the retained Google signer certificate for key {key_id} left its own validity window at unix {expiry} and a fresh set could not be fetched: {error}"
            ),
            None => error,
        })?;
        validate_certificate_set(&certificates)?;
        let encoded = certificates.get(key_id).ok_or_else(|| {
            format!(
                "Google certificate set does not contain signing key {key_id} for the pinned signer"
            )
        })?;
        let certificate = parse_certificate(encoded)?;
        if !certificate.covers(local_now) {
            return Err(format!(
                "the Google signer certificate for key {key_id} is valid only from unix {} to unix {}, which does not contain this machine's clock reading of unix {local_now}",
                certificate.not_before_unix, certificate.not_after_unix
            ));
        }
        let key = certificate.key;
        let cache = CertificateCache {
            schema: 1,
            contract: CERT_CACHE_CONTRACT.to_string(),
            authority: self.pins.identity(),
            signer: self.pins.signer.clone(),
            fetched_at_unix: local_now,
            certificates,
        };
        let body = encode_record(&cache, MAX_AUTHORITY_RECORD_BYTES)?;
        atomic_replace(&cache_path, &body)?;
        Ok(key)
    }

    fn observe_server_time(
        &self,
        candidate_text: &str,
        candidate: DateTime<Utc>,
    ) -> Result<(), String> {
        // The Tauri state mutex serializes one application process. This
        // filesystem lock also covers a second shell process, keeping the
        // high-water read/compare/replace indivisible across processes.
        let _lock = acquire_authority_lock(&self.authority_dir)?;
        let path = self.authority_dir.join(HIGH_WATER_FILE);
        if let Some(raw) = read_bounded_file(&path, MAX_AUTHORITY_RECORD_BYTES)? {
            let prior: ServerTimeHighWater = serde_json::from_slice(&raw)
                .map_err(|error| format!("server-time high-water record is malformed: {error}"))?;
            validate_high_water(&prior, &self.pins)?;
            let previous = parse_timestamp(&prior.server_time, "stored server-time high-water")?;
            if candidate < previous {
                return Err(format!(
                    "edge authority server time rolled back below the stored high-water mark ({})",
                    prior.server_time
                ));
            }
            if candidate == previous {
                return Ok(());
            }
        }
        let record = ServerTimeHighWater {
            schema: 1,
            contract: HIGH_WATER_CONTRACT.to_string(),
            authority: self.pins.identity(),
            signer: self.pins.signer.clone(),
            issuer: self.pins.issuer.clone(),
            audience: self.pins.audience.clone(),
            server_time: candidate_text.to_string(),
        };
        let body = encode_record(&record, MAX_AUTHORITY_RECORD_BYTES)?;
        atomic_replace(&path, &body)
    }
}

trait CertificateSource {
    fn fetch(&self, signer: &str) -> Result<BTreeMap<String, String>, String>;
}

trait PublicKeySource {
    fn resolve(&self, key_id: &str) -> Result<RsaPublicKey, String>;
}

struct CachedGoogleKeySource<'a> {
    verifier: &'a AuthorityVerifier,
    source: &'a dyn CertificateSource,
    local_now: i64,
}

impl PublicKeySource for CachedGoogleKeySource<'_> {
    fn resolve(&self, key_id: &str) -> Result<RsaPublicKey, String> {
        self.verifier
            .resolve_public_key(key_id, self.source, self.local_now)
    }
}

struct GoogleCertificateSource;

impl CertificateSource for GoogleCertificateSource {
    fn fetch(&self, signer: &str) -> Result<BTreeMap<String, String>, String> {
        if !valid_service_account(signer) {
            return Err("pinned Google certificate signer is invalid".to_string());
        }
        let escaped = signer.replace('@', "%40");
        let endpoint = format!("{CERT_ENDPOINT}{escaped}");
        let agent = ureq::Agent::config_builder()
            .https_only(true)
            .max_redirects(0)
            .max_redirects_will_error(true)
            .timeout_global(Some(HTTP_OVERALL_TIMEOUT))
            .timeout_per_call(Some(HTTP_OVERALL_TIMEOUT))
            .timeout_connect(Some(HTTP_CONNECT_TIMEOUT))
            .timeout_recv_response(Some(HTTP_READ_TIMEOUT))
            .timeout_recv_body(Some(HTTP_READ_TIMEOUT))
            .user_agent("ds-grid-desktop-edge-authority/1")
            .build()
            .new_agent();
        let response = agent
            .get(&endpoint)
            .call()
            .map_err(|error| format!("Google signer certificate request failed: {error}"))?;
        if response.status().as_u16() != 200 {
            return Err(format!(
                "Google signer certificate endpoint returned {}",
                response.status().as_u16()
            ));
        }
        let mut reader = response.into_body().into_reader();
        let mut raw = Vec::new();
        (&mut reader)
            .take(MAX_CERT_RESPONSE_BYTES + 1)
            .read_to_end(&mut raw)
            .map_err(|error| format!("could not read Google signer certificates: {error}"))?;
        if raw.is_empty() || raw.len() as u64 > MAX_CERT_RESPONSE_BYTES {
            return Err("Google signer certificate response has an invalid size".to_string());
        }
        serde_json::from_slice(&raw)
            .map_err(|error| format!("Google signer certificate response is invalid: {error}"))
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct CertificateCache {
    schema: u8,
    contract: String,
    authority: String,
    signer: String,
    fetched_at_unix: i64,
    certificates: BTreeMap<String, String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ServerTimeHighWater {
    schema: u8,
    contract: String,
    authority: String,
    signer: String,
    issuer: String,
    audience: String,
    server_time: String,
}

/// Whether this retained record is ours and its clock evidence is coherent.
///
/// It deliberately carries no age limit. How long a certificate may be trusted
/// is stated by the certificate, and is checked per key where the key is used;
/// `fetched_at_unix` is kept as clock evidence — a record claiming to have been
/// fetched in this machine's future is rejected — never as an expiry.
fn validate_certificate_cache(
    cache: &CertificateCache,
    pins: &AuthorityPins,
    local_now: i64,
) -> Result<(), String> {
    if cache.schema != 1
        || cache.contract != CERT_CACHE_CONTRACT
        || cache.authority != pins.identity()
        || cache.signer != pins.signer
        || cache.fetched_at_unix < 0
        || cache.fetched_at_unix > local_now
    {
        return Err(
            "retained signer certificates belong to another authority or carry impossible clock evidence"
                .to_string(),
        );
    }
    validate_certificate_set(&cache.certificates)
}

fn validate_certificate_set(certificates: &BTreeMap<String, String>) -> Result<(), String> {
    if certificates.is_empty() || certificates.len() > 16 {
        return Err("Google signer certificate set has an invalid key count".to_string());
    }
    for (key_id, certificate) in certificates {
        if !valid_key_id(key_id) || certificate.len() > MAX_CERTIFICATE_BYTES {
            return Err("Google signer certificate set is malformed".to_string());
        }
        parse_rsa_certificate(certificate)?;
    }
    Ok(())
}

/// A signer certificate as this crate reads it: the RSA key it carries and the
/// validity window its issuer declared for that key.
struct ParsedCertificate {
    key: RsaPublicKey,
    not_before_unix: i64,
    not_after_unix: i64,
}

impl ParsedCertificate {
    /// Whether the issuer's declared window contains this instant.
    fn covers(&self, local_now: i64) -> bool {
        self.not_before_unix <= local_now && local_now < self.not_after_unix
    }
}

fn parse_rsa_certificate(pem: &str) -> Result<RsaPublicKey, String> {
    parse_certificate(pem).map(|certificate| certificate.key)
}

fn parse_certificate(pem: &str) -> Result<ParsedCertificate, String> {
    const BEGIN: &str = "-----BEGIN CERTIFICATE-----";
    const END: &str = "-----END CERTIFICATE-----";
    if pem.len() > MAX_CERTIFICATE_BYTES || pem.contains('\r') {
        return Err("signer x509 certificate PEM is malformed".to_string());
    }
    let lines: Vec<&str> = pem.lines().collect();
    if lines.len() < 3 || lines.first() != Some(&BEGIN) || lines.last() != Some(&END) {
        return Err("signer x509 certificate PEM is malformed".to_string());
    }
    let mut encoded = String::new();
    for line in &lines[1..lines.len() - 1] {
        if line.is_empty()
            || line.len() > 76
            || !line
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'/' | b'='))
        {
            return Err("signer x509 certificate PEM body is malformed".to_string());
        }
        encoded.push_str(line);
    }
    let der = general_purpose::STANDARD
        .decode(encoded.as_bytes())
        .map_err(|_| "signer x509 certificate is not canonical base64".to_string())?;
    if general_purpose::STANDARD.encode(&der) != encoded {
        return Err("signer x509 certificate is not canonical base64".to_string());
    }
    let certificate = Certificate::from_der(&der)
        .map_err(|_| "signer x509 certificate is invalid DER".to_string())?;
    let spki = certificate
        .tbs_certificate
        .subject_public_key_info
        .to_der()
        .map_err(|_| "signer x509 public key could not be encoded".to_string())?;
    let key = RsaPublicKey::from_public_key_der(&spki)
        .map_err(|_| "signer x509 certificate does not contain an RSA key".to_string())?;
    if !(2048..=8192).contains(&key.n().bits()) {
        return Err("signer x509 RSA key has an unsafe modulus size".to_string());
    }
    let validity = &certificate.tbs_certificate.validity;
    let not_before_unix = validity.not_before.to_unix_duration().as_secs() as i64;
    let not_after_unix = validity.not_after.to_unix_duration().as_secs() as i64;
    if not_after_unix <= not_before_unix {
        return Err("signer x509 certificate declares an empty validity window".to_string());
    }
    Ok(ParsedCertificate {
        key,
        not_before_unix,
        not_after_unix,
    })
}

fn validate_high_water(record: &ServerTimeHighWater, pins: &AuthorityPins) -> Result<(), String> {
    if record.schema != 1
        || record.contract != HIGH_WATER_CONTRACT
        || record.authority != pins.identity()
        || record.signer != pins.signer
        || record.issuer != pins.issuer
        || record.audience != pins.audience
    {
        return Err("server-time high-water belongs to a different edge authority".to_string());
    }
    Ok(())
}

fn validate_window(
    issued_at: DateTime<Utc>,
    expires_at: DateTime<Utc>,
    server_time: DateTime<Utc>,
    max_seconds: i64,
    label: &str,
) -> Result<(), String> {
    let lifetime = expires_at.signed_duration_since(issued_at);
    if issued_at > server_time
        || expires_at <= server_time
        || lifetime <= ChronoDuration::zero()
        || lifetime > ChronoDuration::seconds(max_seconds)
    {
        return Err(format!(
            "signed {label} is future-issued, expired, or overlong"
        ));
    }
    Ok(())
}

fn parse_timestamp(value: &str, label: &str) -> Result<DateTime<Utc>, String> {
    if !canonical_utc_timestamp(value) {
        return Err(format!("{label} is not a canonical UTC timestamp"));
    }
    DateTime::parse_from_rfc3339(value)
        .map(|value| value.with_timezone(&Utc))
        .map_err(|_| format!("{label} is not a valid timestamp"))
}

fn canonical_utc_timestamp(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.len() < 20 || bytes.len() > 30 || bytes.last() != Some(&b'Z') {
        return false;
    }
    for (index, byte) in bytes[..19].iter().enumerate() {
        let separator = match index {
            4 | 7 => Some(b'-'),
            10 => Some(b'T'),
            13 | 16 => Some(b':'),
            _ => None,
        };
        if separator.map_or(!byte.is_ascii_digit(), |expected| *byte != expected) {
            return false;
        }
    }
    if bytes.len() == 20 {
        return true;
    }
    bytes[19] == b'.'
        && bytes.len() >= 22
        && bytes[20..bytes.len() - 1].iter().all(u8::is_ascii_digit)
}

fn decode_raw_base64url(value: &str, maximum: usize, label: &str) -> Result<Vec<u8>, String> {
    if value.is_empty()
        || value.len() > maximum.saturating_mul(2)
        || value.len() % 4 == 1
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err(format!("{label} is not raw unpadded base64url"));
    }
    let decoded = general_purpose::URL_SAFE_NO_PAD
        .decode(value)
        .map_err(|_| format!("{label} is not raw unpadded base64url"))?;
    if decoded.is_empty()
        || decoded.len() > maximum
        || general_purpose::URL_SAFE_NO_PAD.encode(&decoded) != value
    {
        return Err(format!("{label} is not canonical raw base64url"));
    }
    Ok(decoded)
}

fn valid_service_account(value: &str) -> bool {
    value.len() <= 254
        && value.ends_with(".iam.gserviceaccount.com")
        && value.bytes().filter(|byte| *byte == b'@').count() == 1
        && value.bytes().all(|byte| {
            byte.is_ascii_lowercase()
                || byte.is_ascii_digit()
                || matches!(byte, b'@' | b'.' | b'_' | b'-')
        })
}

fn valid_issuer(value: &str) -> bool {
    value.starts_with("https://")
        && !value[8..].is_empty()
        && !value[8..].contains('/')
        && value.bytes().all(|byte| {
            byte.is_ascii_lowercase()
                || byte.is_ascii_digit()
                || matches!(byte, b':' | b'/' | b'.' | b'-')
        })
}

fn valid_key_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
}

fn valid_edge_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value.as_bytes()[0].is_ascii_alphanumeric()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
}

fn valid_network_reporter_grant_id(value: &str) -> bool {
    value.strip_prefix("grant_").is_some_and(|suffix| {
        suffix.len() == 40
            && suffix
                .bytes()
                .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
    })
}

fn canonical_uuid_v4(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 36
        && bytes[8] == b'-'
        && bytes[13] == b'-'
        && bytes[18] == b'-'
        && bytes[23] == b'-'
        && bytes[14] == b'4'
        && matches!(bytes[19], b'8' | b'9' | b'a' | b'b')
        && bytes.iter().enumerate().all(|(index, byte)| {
            matches!(index, 8 | 13 | 18 | 23)
                || byte.is_ascii_digit()
                || matches!(byte, b'a'..=b'f')
        })
}

fn valid_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
}

/// The individual outputs a signed desktop work grant may carry.
///
/// The geospatial pair the desktop engine has always produced under a grant,
/// and every tabular output (`report_formats::TABULAR`): `xlsx` and the two
/// tabular JSON documents, `network_information` and `voltage_drop`. ds-brain
/// mints desktop grants from the same policy and already admits
/// `network_information` there; its validator warns that a signed grant must
/// not keep "a second, obsolete three-format interpretation" of that policy,
/// which is what this list was (shp/kmz/xlsx, at most three) until
/// `voltage_drop` joined the default plan and made a default export four.
///
/// Order, output id and content type are the kernel's report output policy
/// (`report_formats::INDIVIDUAL` and `report_formats::output`), not a copy.
/// Prints and `gpkg` are not admitted here: a print output carries
/// paper/presentation fields this closed schema does not hold.
pub const EDGE_INDIVIDUAL_OUTPUTS: &[&str] =
    &["shp", "kmz", "xlsx", "network_information", "voltage_drop"];

fn valid_individual_outputs(outputs: &[IndividualExecutionOutput]) -> bool {
    use ds_command_kernel::report_formats;
    // Each admitted output at most once, in policy order, so no inventory is
    // longer than the admitted set itself.
    if outputs.is_empty() || outputs.len() > EDGE_INDIVIDUAL_OUTPUTS.len() {
        return false;
    }
    let mut previous = None;
    for output in outputs {
        let format = output.format.as_str();
        let Some(position) = report_formats::INDIVIDUAL
            .iter()
            .position(|known| *known == format)
            .filter(|_| EDGE_INDIVIDUAL_OUTPUTS.contains(&format))
        else {
            return false;
        };
        let Ok(definition) = report_formats::output(format) else {
            return false;
        };
        if output.output_id != definition.output_id
            || output.format != definition.format
            || output.content_type != definition.content_type
            || previous.is_some_and(|value| value >= position)
        {
            return false;
        }
        previous = Some(position);
    }
    true
}

fn valid_reporter_release(value: &str) -> bool {
    value
        .strip_prefix("ds-network-reporter@0.1.0+")
        .is_some_and(|source| {
            source.len() == 40
                && source
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
        })
}

fn valid_transformer(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.is_empty() || bytes.len() > 121 {
        return false;
    }
    let valid_tail = |tail: &[u8]| {
        tail.iter()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'_')
    };
    (bytes[0].is_ascii_lowercase() && valid_tail(&bytes[1..]))
        || (bytes.len() >= 2
            && bytes[0] == b'_'
            && bytes[1].is_ascii_digit()
            && valid_tail(&bytes[2..]))
}

fn bounded_text(value: &str, maximum: usize) -> bool {
    !value.is_empty()
        && value.len() <= maximum
        && value.trim() == value
        && !value.chars().any(char::is_control)
}

fn unix_now() -> Result<i64, String> {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| "desktop system clock predates the Unix epoch".to_string())?
        .as_secs();
    i64::try_from(seconds).map_err(|_| "desktop system clock cannot be represented".to_string())
}

fn encode_record<T: Serialize>(value: &T, maximum: u64) -> Result<Vec<u8>, String> {
    let mut body = serde_json::to_vec(value)
        .map_err(|error| format!("could not encode edge authority record: {error}"))?;
    body.push(b'\n');
    if body.is_empty() || body.len() as u64 > maximum {
        return Err("edge authority record exceeds its size bound".to_string());
    }
    Ok(body)
}

fn validate_authority_directory(path: &Path) -> Result<(), String> {
    let metadata = std::fs::symlink_metadata(path)
        .map_err(|error| format!("could not inspect edge authority directory: {error}"))?;
    if metadata_is_link_or_reparse(&metadata) || !metadata.is_dir() {
        return Err("edge authority directory is linked, reparsed, or unavailable".to_string());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o077 != 0 {
            return Err("edge authority directory permissions are not private".to_string());
        }
    }
    Ok(())
}

#[cfg(unix)]
fn metadata_is_link_or_reparse(metadata: &std::fs::Metadata) -> bool {
    metadata.file_type().is_symlink()
}

#[cfg(windows)]
fn metadata_is_link_or_reparse(metadata: &std::fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    metadata.file_attributes() & 0x0000_0400 != 0
}

fn read_bounded_file(path: &Path, maximum: u64) -> Result<Option<Vec<u8>>, String> {
    let before = match std::fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("could not inspect edge authority record: {error}")),
    };
    if metadata_is_link_or_reparse(&before)
        || !before.is_file()
        || before.len() == 0
        || before.len() > maximum
    {
        return Err("edge authority record is linked, non-file, or unbounded".to_string());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if before.permissions().mode() & 0o077 != 0 {
            return Err("edge authority record permissions are not private".to_string());
        }
    }
    let parent = path
        .parent()
        .ok_or_else(|| "edge authority record has no parent directory".to_string())?;
    validate_authority_directory(parent)?;
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.share_mode(0x0000_0001).custom_flags(0x0020_0000);
    }
    let mut file = options
        .open(path)
        .map_err(|error| format!("could not open edge authority record: {error}"))?;
    let held = file
        .metadata()
        .map_err(|error| format!("could not inspect held edge authority record: {error}"))?;
    if metadata_is_link_or_reparse(&held)
        || !held.is_file()
        || held.len() != before.len()
        || held.len() > maximum
    {
        return Err("edge authority record changed identity while it was opened".to_string());
    }
    #[cfg(unix)]
    if !same_file_identity(&before, &held) {
        return Err("edge authority record changed identity while it was opened".to_string());
    }
    #[cfg(windows)]
    if !opened_file_matches_path(&file, path)? {
        return Err("edge authority record changed identity while it was opened".to_string());
    }
    let mut raw = Vec::with_capacity(held.len() as usize);
    (&mut file)
        .take(maximum + 1)
        .read_to_end(&mut raw)
        .map_err(|error| format!("could not read edge authority record: {error}"))?;
    if raw.len() as u64 != held.len() || raw.len() as u64 > maximum {
        return Err("edge authority record changed size while it was read".to_string());
    }
    let after = std::fs::symlink_metadata(path)
        .map_err(|error| format!("edge authority record changed while it was read: {error}"))?;
    if metadata_is_link_or_reparse(&after) {
        return Err("edge authority record changed identity while it was read".to_string());
    }
    #[cfg(unix)]
    if !same_file_identity(&held, &after) {
        return Err("edge authority record changed identity while it was read".to_string());
    }
    #[cfg(windows)]
    if !opened_file_matches_path(&file, path)? {
        return Err("edge authority record changed identity while it was read".to_string());
    }
    Ok(Some(raw))
}

#[cfg(unix)]
fn same_file_identity(left: &std::fs::Metadata, right: &std::fs::Metadata) -> bool {
    use std::os::unix::fs::MetadataExt;
    left.dev() == right.dev() && left.ino() == right.ino()
}

#[cfg(windows)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct FileIdentity {
    volume_serial_number: u32,
    file_index: u64,
}

#[cfg(windows)]
fn file_identity(file: &File) -> Result<FileIdentity, String> {
    use std::os::windows::io::AsRawHandle;
    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::Storage::FileSystem::{
        GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION,
    };

    let mut information = BY_HANDLE_FILE_INFORMATION::default();
    // SAFETY: `file` owns a live Win32 handle and `information` is writable
    // storage with the exact layout required by GetFileInformationByHandle.
    unsafe { GetFileInformationByHandle(HANDLE(file.as_raw_handle()), &mut information) }
        .map_err(|error| format!("could not inspect edge authority file identity: {error}"))?;
    Ok(FileIdentity {
        volume_serial_number: information.dwVolumeSerialNumber,
        file_index: (u64::from(information.nFileIndexHigh) << 32)
            | u64::from(information.nFileIndexLow),
    })
}

#[cfg(windows)]
fn opened_file_matches_path(file: &File, path: &Path) -> Result<bool, String> {
    use std::os::windows::fs::OpenOptionsExt;

    let mut options = OpenOptions::new();
    options
        .read(true)
        .share_mode(0x0000_0001) // FILE_SHARE_READ
        .custom_flags(0x0020_0000); // FILE_FLAG_OPEN_REPARSE_POINT
    let visible = options
        .open(path)
        .map_err(|error| format!("could not reopen edge authority record: {error}"))?;
    let metadata = visible
        .metadata()
        .map_err(|error| format!("could not inspect reopened edge authority record: {error}"))?;
    if metadata_is_link_or_reparse(&metadata) || !metadata.is_file() {
        return Ok(false);
    }
    Ok(file_identity(file)? == file_identity(&visible)?)
}

#[cfg(unix)]
#[derive(Debug)]
struct AuthorityLock {
    file: File,
}

#[cfg(unix)]
impl Drop for AuthorityLock {
    fn drop(&mut self) {
        use std::os::fd::AsRawFd;

        // SAFETY: `file` remains an owned live descriptor. Closing it would
        // release the advisory lock too; the explicit unlock documents the
        // exact end of the server-time compare/replace critical section.
        unsafe {
            libc::flock(self.file.as_raw_fd(), libc::LOCK_UN);
        }
    }
}

#[cfg(windows)]
#[derive(Debug)]
struct AuthorityLock {
    path: PathBuf,
    file: Option<File>,
}

#[cfg(windows)]
impl Drop for AuthorityLock {
    fn drop(&mut self) {
        drop(self.file.take());
        let _ = std::fs::remove_file(&self.path);
    }
}

/// Name the holder in the contention refusal.
///
/// This lock gates every publication promotion, so when it refuses, the pump
/// stops. The sentence used to name nobody and nothing to run, which is how a
/// stopped queue turned into an afternoon of guessing. The marker's own owner
/// evidence is the pid it already writes; liveness is deliberately NOT decided
/// here — breaking a lock needs the paired pid/heartbeat/boot evidence that
/// `ds_report_artifacts::confined_fs::LockOwner` owns, and duplicating half of
/// that rule in a message would invite someone to act on it.
fn busy_authority_lock(path: &Path) -> String {
    let holder = std::fs::read_to_string(path)
        .ok()
        .and_then(|body| {
            let first = body.lines().next()?.trim().to_string();
            (!first.is_empty() && first.len() <= 128).then_some(first)
        })
        .map(|owner| format!(" It is held by {owner}."))
        .unwrap_or_default();
    format!(
        "edge authority state is busy; retry the operation.{holder} Run `ds report outbox status` to see whether the publication queue is moving."
    )
}

#[cfg(unix)]
fn acquire_authority_lock_with_attempts(
    directory: &Path,
    attempts: usize,
) -> Result<AuthorityLock, String> {
    use std::os::fd::AsRawFd;
    use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};

    validate_authority_directory(directory)?;
    let path = directory.join(AUTHORITY_LOCK_FILE);
    match std::fs::symlink_metadata(&path) {
        Ok(metadata) => {
            if metadata_is_link_or_reparse(&metadata)
                || !metadata.is_file()
                || metadata.permissions().mode() & 0o077 != 0
            {
                return Err("edge authority lock is linked, non-file, or non-private".to_string());
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(format!("could not inspect edge authority lock: {error}")),
    }
    let mut options = OpenOptions::new();
    options
        .read(true)
        .write(true)
        .create(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
    let mut file = options
        .open(&path)
        .map_err(|error| format!("could not open edge authority lock: {error}"))?;
    let held = file
        .metadata()
        .map_err(|error| format!("could not inspect held edge authority lock: {error}"))?;
    let visible = std::fs::symlink_metadata(&path)
        .map_err(|error| format!("edge authority lock changed while opening: {error}"))?;
    if !held.is_file()
        || held.permissions().mode() & 0o077 != 0
        || metadata_is_link_or_reparse(&visible)
        || !same_file_identity(&held, &visible)
    {
        return Err("edge authority lock changed identity while it was opened".to_string());
    }

    for attempt in 0..attempts {
        // SAFETY: `file` owns a live descriptor. flock is released by the
        // kernel on process exit, so age is never used to break a live lock.
        if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0 {
            let after = std::fs::symlink_metadata(&path)
                .map_err(|error| format!("edge authority lock changed while waiting: {error}"))?;
            if metadata_is_link_or_reparse(&after)
                || held.dev() != after.dev()
                || held.ino() != after.ino()
            {
                return Err("edge authority lock changed identity while waiting".to_string());
            }
            file.set_len(0)
                .and_then(|_| writeln!(file, "{}", std::process::id()))
                .and_then(|_| file.sync_data())
                .map_err(|error| format!("could not persist edge authority lock owner: {error}"))?;
            return Ok(AuthorityLock { file });
        }
        let error = std::io::Error::last_os_error();
        let code = error.raw_os_error();
        if code != Some(libc::EWOULDBLOCK) && code != Some(libc::EAGAIN) {
            return Err(format!("could not acquire edge authority lock: {error}"));
        }
        if attempt + 1 < attempts {
            std::thread::sleep(AUTHORITY_LOCK_RETRY);
        }
    }
    Err(busy_authority_lock(&path))
}

#[cfg(unix)]
fn acquire_authority_lock(directory: &Path) -> Result<AuthorityLock, String> {
    acquire_authority_lock_with_attempts(directory, AUTHORITY_LOCK_ATTEMPTS)
}

#[cfg(windows)]
fn acquire_authority_lock_with_attempts(
    directory: &Path,
    attempts: usize,
) -> Result<AuthorityLock, String> {
    use std::os::windows::fs::OpenOptionsExt;

    validate_authority_directory(directory)?;
    let path = directory.join(AUTHORITY_LOCK_FILE);
    for attempt in 0..attempts {
        let mut options = OpenOptions::new();
        options
            .write(true)
            .create_new(true)
            // A live holder denies delete. After a crash Windows closes the
            // handle, allowing only the abandoned marker to be reclaimed.
            .share_mode(0x0000_0001)
            .custom_flags(0x0020_0000);
        match options.open(&path) {
            Ok(mut file) => {
                writeln!(file, "{}", std::process::id())
                    .and_then(|_| file.sync_data())
                    .map_err(|error| {
                        format!("could not persist edge authority lock owner: {error}")
                    })?;
                return Ok(AuthorityLock {
                    path,
                    file: Some(file),
                });
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                let metadata = match std::fs::symlink_metadata(&path) {
                    Ok(metadata) => metadata,
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                    Err(error) => {
                        return Err(format!("could not inspect edge authority lock: {error}"));
                    }
                };
                if metadata_is_link_or_reparse(&metadata) || !metadata.is_file() {
                    return Err("edge authority lock is linked, reparsed, or non-file".to_string());
                }
                match std::fs::remove_file(&path) {
                    Ok(()) => continue,
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                    Err(_) => {}
                }
                if attempt + 1 < attempts {
                    std::thread::sleep(AUTHORITY_LOCK_RETRY);
                }
            }
            Err(error) => return Err(format!("could not acquire edge authority lock: {error}")),
        }
    }
    Err(busy_authority_lock(&path))
}

#[cfg(windows)]
fn acquire_authority_lock(directory: &Path) -> Result<AuthorityLock, String> {
    acquire_authority_lock_with_attempts(directory, AUTHORITY_LOCK_ATTEMPTS)
}

#[cfg(unix)]
fn atomic_replace(path: &Path, body: &[u8]) -> Result<(), String> {
    use std::ffi::CString;
    use std::os::fd::{AsRawFd, FromRawFd};
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::fs::MetadataExt;

    let parent = path
        .parent()
        .ok_or_else(|| "edge authority record has no parent directory".to_string())?;
    validate_authority_directory(parent)?;
    if let Ok(existing) = std::fs::symlink_metadata(path) {
        if metadata_is_link_or_reparse(&existing) || !existing.is_file() {
            return Err("edge authority record target is linked or non-file".to_string());
        }
    }
    let before = std::fs::symlink_metadata(parent)
        .map_err(|error| format!("could not inspect edge authority directory: {error}"))?;
    let parent_name = CString::new(parent.as_os_str().as_bytes())
        .map_err(|_| "edge authority directory contains a NUL byte".to_string())?;
    // SAFETY: parent_name is NUL terminated and ownership is transferred.
    let descriptor = unsafe {
        libc::open(
            parent_name.as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    };
    if descriptor < 0 {
        return Err(format!(
            "could not hold edge authority directory: {}",
            std::io::Error::last_os_error()
        ));
    }
    // SAFETY: descriptor is newly owned.
    let directory = unsafe { File::from_raw_fd(descriptor) };
    let held = directory
        .metadata()
        .map_err(|error| format!("could not inspect held edge authority directory: {error}"))?;
    if !held.is_dir() || before.dev() != held.dev() || before.ino() != held.ino() {
        return Err("edge authority directory changed while it was opened".to_string());
    }
    let final_name = CString::new(
        path.file_name()
            .ok_or_else(|| "edge authority record has no filename".to_string())?
            .as_bytes(),
    )
    .map_err(|_| "edge authority filename contains a NUL byte".to_string())?;
    let mut temporary_name: Option<CString> = None;
    let result = (|| {
        let mut temporary_file = None;
        for _ in 0..8 {
            let candidate = CString::new(format!(
                ".edge-authority.{:016x}{:016x}.tmp",
                rand::random::<u64>(),
                rand::random::<u64>()
            ))
            .expect("generated authority temp name contains no NUL");
            // SAFETY: names are relative to the held directory descriptor.
            let temporary_descriptor = unsafe {
                libc::openat(
                    directory.as_raw_fd(),
                    candidate.as_ptr(),
                    libc::O_WRONLY
                        | libc::O_CREAT
                        | libc::O_EXCL
                        | libc::O_NOFOLLOW
                        | libc::O_CLOEXEC,
                    0o600,
                )
            };
            if temporary_descriptor >= 0 {
                temporary_name = Some(candidate);
                // SAFETY: temporary_descriptor is newly owned.
                temporary_file = Some(unsafe { File::from_raw_fd(temporary_descriptor) });
                break;
            }
            let error = std::io::Error::last_os_error();
            if error.kind() != std::io::ErrorKind::AlreadyExists {
                return Err(format!(
                    "could not create edge authority staging file: {error}"
                ));
            }
        }
        let mut file = temporary_file
            .ok_or_else(|| "could not allocate edge authority staging file".to_string())?;
        file.write_all(body)
            .and_then(|_| file.sync_all())
            .map_err(|error| format!("could not persist edge authority record: {error}"))?;
        drop(file);
        let temporary = temporary_name
            .as_ref()
            .expect("staging name accompanies the held file");
        // SAFETY: both names are beneath the same held directory.
        if unsafe {
            libc::renameat(
                directory.as_raw_fd(),
                temporary.as_ptr(),
                directory.as_raw_fd(),
                final_name.as_ptr(),
            )
        } != 0
        {
            return Err(format!(
                "could not activate edge authority record: {}",
                std::io::Error::last_os_error()
            ));
        }
        temporary_name = None;
        directory
            .sync_all()
            .map_err(|error| format!("could not sync edge authority directory: {error}"))?;
        let after = std::fs::symlink_metadata(parent)
            .map_err(|error| format!("edge authority directory changed: {error}"))?;
        if metadata_is_link_or_reparse(&after)
            || after.dev() != held.dev()
            || after.ino() != held.ino()
        {
            return Err("edge authority directory changed during activation".to_string());
        }
        Ok::<(), String>(())
    })();
    if let Some(temporary) = temporary_name.as_ref() {
        // SAFETY: cleanup remains relative to the held directory.
        unsafe {
            libc::unlinkat(directory.as_raw_fd(), temporary.as_ptr(), 0);
        }
    }
    result
}

#[cfg(windows)]
fn atomic_replace(path: &Path, body: &[u8]) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;
    use std::os::windows::fs::OpenOptionsExt;
    use windows::core::PCWSTR;
    use windows::Win32::Storage::FileSystem::{
        MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
    };

    let parent = path
        .parent()
        .ok_or_else(|| "edge authority record has no parent directory".to_string())?;
    validate_authority_directory(parent)?;
    if let Ok(existing) = std::fs::symlink_metadata(path) {
        if metadata_is_link_or_reparse(&existing) || !existing.is_file() {
            return Err(
                "edge authority record target is linked, reparsed, or non-file".to_string(),
            );
        }
    }
    let mut temporary_path = None;
    let result = (|| {
        let mut temporary_file = None;
        for _ in 0..8 {
            let candidate = parent.join(format!(
                ".edge-authority.{:016x}{:016x}.tmp",
                rand::random::<u64>(),
                rand::random::<u64>()
            ));
            let mut options = OpenOptions::new();
            options.write(true).create_new(true).share_mode(0x0000_0001);
            match options.open(&candidate) {
                Ok(file) => {
                    temporary_path = Some(candidate);
                    temporary_file = Some(file);
                    break;
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => {
                    return Err(format!(
                        "could not create edge authority staging file: {error}"
                    ))
                }
            }
        }
        let mut file = temporary_file
            .ok_or_else(|| "could not allocate edge authority staging file".to_string())?;
        file.write_all(body)
            .and_then(|_| file.sync_all())
            .map_err(|error| format!("could not persist edge authority record: {error}"))?;
        drop(file);
        let temporary = temporary_path
            .as_ref()
            .expect("staging path accompanies held file");
        let temporary_wide: Vec<u16> = temporary
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        let final_wide: Vec<u16> = path
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        // SAFETY: UTF-16 buffers are NUL terminated and remain live.
        unsafe {
            MoveFileExW(
                PCWSTR(temporary_wide.as_ptr()),
                PCWSTR(final_wide.as_ptr()),
                MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
            )
        }
        .map_err(|error| format!("could not activate edge authority record: {error}"))?;
        temporary_path = None;
        validate_authority_directory(parent)
    })();
    if let Some(temporary) = temporary_path {
        let _ = std::fs::remove_file(temporary);
    }
    result
}

#[cfg(test)]
mod tests;
