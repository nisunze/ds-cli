use super::*;
use rand_chacha::ChaCha20Rng;
use rand_core::SeedableRng;
use rsa::pkcs1v15::SigningKey;
use rsa::signature::{SignatureEncoding as _, Signer as _};
use rsa::RsaPrivateKey;
use std::cell::Cell;
use std::collections::HashSet;
use std::sync::OnceLock;

const INSTALL_ID: &str = "550e8400-e29b-41d4-a716-446655440000";
const OWNER: &str = "owner-1";
const RELEASE: &str = "ds-network-reporter@0.1.0+aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const FINGERPRINT: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const ROOM_CONTENT_SHA256: &str =
    "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
const CERT_KEY_ID: &str = "78c62ed866e7dcf7035cf094f92e4a66376a9608";
pub(super) const CERTIFICATE: &str = "-----BEGIN CERTIFICATE-----\n\
MIIDNTCCAh2gAwIBAgIJANVQ/t+RZTj1MA0GCSqGSIb3DQEBBQUAMD0xOzA5BgNV\n\
BAMMMjI2MTI1Nzc0NjMwMS1jb21wdXRlLmRldmVsb3Blci5nc2VydmljZWFjY291\n\
bnQuY29tMB4XDTI2MDQwMjE2NTY0NFoXDTI3MDQwMjE2NTY0NFowPTE7MDkGA1UE\n\
AwwyMjYxMjU3NzQ2MzAxLWNvbXB1dGUuZGV2ZWxvcGVyLmdzZXJ2aWNlYWNjb3Vu\n\
dC5jb20wggEiMA0GCSqGSIb3DQEBAQUAA4IBDwAwggEKAoIBAQCmRqjhDg5HPO0k\n\
rdg2Ow7jVCP+A3sK4VQ3RX2pc4wn/keEHHbZR/cvqFeX+gzOAM894si5s0ukfbnU\n\
oufnUFpWcDExeNBuu8SfNsIlrYIiA8byh6+BsEQN58IaTxmhnq/C8fO/RH7byQZ9\n\
KlXvJYs9lPR3/rpfTKrjUtwQ9VqPQNLzRk7MfeOPuMxfwHhilEm2Kqo4d0C+hb8F\n\
OEY3RN4Kij4LH5WWwrI/l+VWnasHFBO83C6mpGpk/nJ5ePxkMbZFJCjbwe0dvKHq\n\
cABdpRDkOD7G8PqeMus1Vq1Nk4g2NPdLyhOeSAPLDfLP2xaEiAnluN3GYGADFv5G\n\
Cbtk7TXNAgMBAAGjODA2MAwGA1UdEwEB/wQCMAAwDgYDVR0PAQH/BAQDAgeAMBYG\n\
A1UdJQEB/wQMMAoGCCsGAQUFBwMCMA0GCSqGSIb3DQEBBQUAA4IBAQBbkj7sw0H+\n\
eWHjVM1CnVpqbSMElLn4kDIIUrqYM2B56h686zzGPhoM2J77gqp1tECh0wDd+KlK\n\
IID98smR3s5wH/cxV++KbRPVN4H+hIfP47WzIMHcPM8rVYPx0qzpfJ9r//ndE9ou\n\
eoKQVrBSyaj9o9l3kSHgOAXawUo4+nAu5y9mZPSxY/TqYeOxgtQDypD+YXCEsif4\n\
atzUIbBXk2uJ0KLzkTsQsQFOiW9yQ7CkKhiz+XoKfhwr2zXBkrHWkgDrgS/wZcgI\n\
oFIGFzwGyNl/fJmaFARl/IbyWSB2tk2PRDIH8FFLR02+BeUWHMv/hdMD+qXGIa4Q\n\
oq0QHxwAKo2t\n\
-----END CERTIFICATE-----\n";

pub(super) fn private_key() -> RsaPrivateKey {
    static KEY: OnceLock<RsaPrivateKey> = OnceLock::new();
    KEY.get_or_init(|| {
        RsaPrivateKey::new(&mut ChaCha20Rng::from_seed([0x5a; 32]), 2048)
            .expect("deterministic test RSA key")
    })
    .clone()
}

struct MemoryKey {
    key_id: &'static str,
    key: RsaPublicKey,
}

impl PublicKeySource for MemoryKey {
    fn resolve(&self, key_id: &str) -> Result<RsaPublicKey, String> {
        if key_id != self.key_id {
            return Err(format!("missing test key {key_id}"));
        }
        Ok(self.key.clone())
    }
}

struct TestRoot(PathBuf);

impl TestRoot {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "ds-edge-authority-test-{:016x}{:016x}",
            rand::random::<u64>(),
            rand::random::<u64>()
        ));
        std::fs::create_dir(&path).expect("create test authority root");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700))
                .expect("secure test authority root");
        }
        Self(path)
    }
}

impl Drop for TestRoot {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

pub(super) fn pins() -> AuthorityPins {
    AuthorityPins::new(
        "canary",
        "edge-capability-signer-canary@edge-signing-test.iam.gserviceaccount.com",
        "https://ds-brain-canary-261257746301.us-east1.run.app",
        "urn:rw.datasolutions:desktop-edge",
    )
    .unwrap()
}

#[test]
fn authority_pins_reject_invalid_lane_and_shape() {
    assert!(AuthorityPins::new(
        "local",
        "edge-capability-signer@edge-signing-test.iam.gserviceaccount.com",
        "https://ds-brain-canary-261257746301.us-east1.run.app",
        "urn:rw.datasolutions:desktop-edge",
    )
    .is_err());
    assert!(AuthorityPins::new(
        "stable",
        "not-a-service-account",
        "https://attacker.example",
        "urn:rw.datasolutions:desktop-edge",
    )
    .is_err());
}

#[test]
fn anonymous_principal_requires_org_to_be_absent_not_null() {
    assert_eq!(
        serde_json::from_str::<Principal>(r#"{"uid":""}"#).unwrap(),
        Principal {
            uid: String::new(),
            org: None,
        }
    );
    assert!(serde_json::from_str::<Principal>(r#"{"uid":"","org":null}"#).is_err());
}

fn verifier(root: &TestRoot) -> AuthorityVerifier {
    AuthorityVerifier::new(&root.0, pins()).unwrap()
}

fn keys() -> MemoryKey {
    let private = private_key();
    MemoryKey {
        key_id: "test-key",
        key: private.to_public_key(),
    }
}

#[test]
fn os_version_is_coarse_bounded_text_not_a_hardware_identifier() {
    assert_eq!(
        bounded_os_version("Windows", "11 23H2").unwrap(),
        "Windows 11 23H2"
    );
    assert!(bounded_os_version("Linux", "Unknown").is_err());
    assert!(bounded_os_version("Linux\nserial", "6.1").is_err());
    assert!(bounded_os_version("Linux", &"x".repeat(121)).is_err());
}

fn signed_bytes<T: Serialize>(domain: &str, payload: &T) -> (String, SignatureDeclaration) {
    let mut bytes = domain.as_bytes().to_vec();
    bytes.push(0);
    bytes.extend(serde_json::to_vec(payload).unwrap());
    let signature = SigningKey::<Sha256>::new(private_key()).sign(&bytes);
    (
        general_purpose::URL_SAFE_NO_PAD.encode(&bytes),
        SignatureDeclaration {
            algorithm: "RS256".to_string(),
            signer: pins().signer,
            key_id: "test-key".to_string(),
            value: general_purpose::URL_SAFE_NO_PAD.encode(signature.to_vec()),
        },
    )
}

pub(super) fn install_payload() -> InstallLeasePayload {
    InstallLeasePayload {
        schema: 1,
        issuer: pins().issuer,
        audience: pins().audience,
        install_id: INSTALL_ID.to_string(),
        device: "windows".to_string(),
        arch: "x86_64".to_string(),
        app_version: "0.1.2".to_string(),
        channel: "canary".to_string(),
        principal: Principal {
            uid: OWNER.to_string(),
            org: Some("org-1".to_string()),
        },
        status: "active".to_string(),
        min_supported_version: "0.1.0".to_string(),
        issued_at: "2026-08-18T12:00:00Z".to_string(),
        expires_at: "2026-08-25T12:00:00Z".to_string(),
    }
}

pub(super) fn install_request(
    payload: InstallLeasePayload,
    server_time: &str,
) -> VerifyInstallRequest {
    let (signed_payload, signature) = signed_bytes(INSTALL_DOMAIN, &payload);
    VerifyInstallRequest {
        server_time: server_time.to_string(),
        expected: ExpectedInstall {
            install_id: INSTALL_ID.to_string(),
            device: "windows".to_string(),
            arch: "x86_64".to_string(),
            app_version: "0.1.2".to_string(),
            channel: "canary".to_string(),
            owner_uid: Some(OWNER.to_string()),
            status: payload.status.clone(),
            min_supported_version: payload.min_supported_version.clone(),
            lease_expires_at: payload.expires_at.clone(),
        },
        capability: InstallLeaseCapability {
            schema: payload.schema,
            issuer: payload.issuer,
            audience: payload.audience,
            install_id: payload.install_id,
            device: payload.device,
            arch: payload.arch,
            app_version: payload.app_version,
            channel: payload.channel,
            principal: payload.principal,
            status: payload.status,
            min_supported_version: payload.min_supported_version,
            issued_at: payload.issued_at,
            expires_at: payload.expires_at,
            signed_payload,
            signature,
        },
    }
}

fn work_payload() -> WorkGrantPayload {
    WorkGrantPayload {
        schema: 1,
        issuer: pins().issuer,
        audience: pins().audience,
        grant_id: "grant_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_string(),
        install_id: INSTALL_ID.to_string(),
        device: "windows".to_string(),
        principal: Principal {
            uid: OWNER.to_string(),
            org: Some("org-1".to_string()),
        },
        scope: GrantScope {
            ds_project: "project-1".to_string(),
            capabilities: vec!["design.report".to_string()],
            engines: vec![GrantEngine {
                name: "ds-network-reporter".to_string(),
                release: RELEASE.to_string(),
            }],
        },
        snapshot_base: BTreeMap::from([("export-_001_main".to_string(), FINGERPRINT.to_string())]),
        individual_execution: individual_execution(),
        issued_at: "2026-08-18T12:00:02Z".to_string(),
        expires_at: "2026-08-19T12:00:02Z".to_string(),
    }
}

fn individual_execution() -> IndividualExecution {
    IndividualExecution {
        transformer: "_001_main".to_string(),
        revision: 7,
        input_base_fingerprint: FINGERPRINT.to_string(),
        room_content_sha256: ROOM_CONTENT_SHA256.to_string(),
        outputs: vec![
            IndividualExecutionOutput {
                output_id: "shp".to_string(),
                format: "shp".to_string(),
                content_type: "application/zip".to_string(),
            },
            IndividualExecutionOutput {
                output_id: "kmz".to_string(),
                format: "kmz".to_string(),
                content_type: "application/vnd.google-earth.kmz".to_string(),
            },
            IndividualExecutionOutput {
                output_id: "xlsx".to_string(),
                format: "xlsx".to_string(),
                content_type: "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet"
                    .to_string(),
            },
        ],
    }
}

fn work_request(payload: WorkGrantPayload, server_time: &str) -> VerifyWorkRequest {
    let (signed_payload, signature) = signed_bytes(WORK_DOMAIN, &payload);
    VerifyWorkRequest {
        server_time: server_time.to_string(),
        snapshot_manifest: NetworkReporterSnapshotManifest {
            schema: 1,
            engine: SnapshotEngine {
                name: "ds-network-reporter".to_string(),
                release: RELEASE.to_string(),
            },
            transformers: vec![SnapshotTransformer {
                name: "_001_main".to_string(),
                revision: 7,
                input_base_fingerprint: FINGERPRINT.to_string(),
            }],
            individual_execution: payload.individual_execution.clone(),
        },
        expected: ExpectedWork {
            install_id: INSTALL_ID.to_string(),
            device: "windows".to_string(),
            owner_uid: OWNER.to_string(),
            organization_id: Some("org-1".to_string()),
            ds_project: "project-1".to_string(),
            engine_release: RELEASE.to_string(),
            transformer: "_001_main".to_string(),
            lease_expires_at: "2026-09-01T12:00:00Z".to_string(),
        },
        capability: WorkGrantCapability {
            schema: payload.schema,
            issuer: payload.issuer,
            audience: payload.audience,
            grant_id: payload.grant_id,
            install_id: payload.install_id,
            device: payload.device,
            principal: payload.principal,
            scope: WorkGrantScope {
                ds_project: payload.scope.ds_project,
                capabilities: payload.scope.capabilities,
                engines: payload
                    .scope
                    .engines
                    .into_iter()
                    .map(|engine| WorkGrantEngine {
                        name: engine.name,
                        release: engine.release,
                    })
                    .collect(),
            },
            snapshot_base: payload.snapshot_base,
            individual_execution: payload.individual_execution,
            issued_at: payload.issued_at,
            expires_at: payload.expires_at,
            signed_payload,
            signature,
        },
    }
}

fn artifact_read_payload() -> ArtifactReadEntitlementPayload {
    ArtifactReadEntitlementPayload {
        schema: 1,
        issuer: pins().issuer,
        audience: pins().audience,
        install_id: INSTALL_ID.to_string(),
        device: "windows".to_string(),
        principal: Principal {
            uid: OWNER.to_string(),
            org: Some("org-1".to_string()),
        },
        scope: GrantScope {
            ds_project: "project-1".to_string(),
            capabilities: vec!["design.report".to_string()],
            engines: vec![GrantEngine {
                name: "ds-network-reporter".to_string(),
                release: RELEASE.to_string(),
            }],
        },
        grant_id: "grant_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_string(),
        batch_id: "d".repeat(64),
        individual_execution: individual_execution(),
        issued_at: "2026-08-18T12:00:04Z".to_string(),
        expires_at: "2026-08-18T12:05:04Z".to_string(),
    }
}

fn artifact_read_request(
    payload: ArtifactReadEntitlementPayload,
    server_time: &str,
) -> VerifyArtifactReadRequest {
    let (signed_payload, signature) = signed_bytes(ARTIFACT_READ_DOMAIN, &payload);
    VerifyArtifactReadRequest {
        server_time: server_time.to_string(),
        expected: ExpectedArtifactRead {
            install_id: INSTALL_ID.to_string(),
            device: "windows".to_string(),
            owner_uid: OWNER.to_string(),
            organization_id: Some("org-1".to_string()),
            grant_id: payload.grant_id.clone(),
            batch_id: payload.batch_id.clone(),
            lease_expires_at: "2026-09-01T12:00:00Z".to_string(),
        },
        capability: ArtifactReadEntitlementCapability {
            schema: payload.schema,
            issuer: payload.issuer,
            audience: payload.audience,
            install_id: payload.install_id,
            device: payload.device,
            principal: payload.principal,
            scope: WorkGrantScope {
                ds_project: payload.scope.ds_project,
                capabilities: payload.scope.capabilities,
                engines: payload
                    .scope
                    .engines
                    .into_iter()
                    .map(|engine| WorkGrantEngine {
                        name: engine.name,
                        release: engine.release,
                    })
                    .collect(),
            },
            grant_id: payload.grant_id,
            batch_id: payload.batch_id,
            individual_execution: payload.individual_execution,
            issued_at: payload.issued_at,
            expires_at: payload.expires_at,
            signed_payload,
            signature,
        },
    }
}

#[test]
fn verifies_exact_install_signature_and_seeds_high_water() {
    let root = TestRoot::new();
    let verified = verifier(&root)
        .verify_install_with_keys(
            install_request(install_payload(), "2026-08-18T12:00:01Z"),
            &keys(),
        )
        .unwrap();
    assert_eq!(verified.status, VerifiedInstallStatus::Active);
    assert_eq!(verified.owner_uid.as_deref(), Some(OWNER));
    assert!(root.0.join(HIGH_WATER_FILE).is_file());
}

#[test]
fn rejects_bad_signer_domain_key_and_signature() {
    let cases = [
        "signer",
        "domain",
        "key",
        "signature",
        "algorithm",
        "padded",
    ];
    for case in cases {
        let root = TestRoot::new();
        let authority = verifier(&root);
        let mut request = install_request(install_payload(), "2026-08-18T12:00:01Z");
        match case {
            "signer" => {
                request.capability.signature.signer =
                    "other@electrification-rural.iam.gserviceaccount.com".to_string()
            }
            "domain" => {
                let payload = install_payload();
                let (signed, signature) = signed_bytes(WORK_DOMAIN, &payload);
                request.capability.signed_payload = signed;
                request.capability.signature = signature;
            }
            "key" => request.capability.signature.key_id = "missing-key".to_string(),
            "signature" => request.capability.signature.value.replace_range(0..1, "A"),
            "algorithm" => request.capability.signature.algorithm = "PS256".to_string(),
            "padded" => request.capability.signed_payload.push('='),
            _ => unreachable!(),
        }
        assert!(
            authority
                .verify_install_with_keys(request, &keys())
                .is_err(),
            "{case}"
        );
    }
}

#[test]
fn rejects_pinned_issuer_and_audience_drift_even_when_resigned() {
    for mutate in ["issuer", "audience"] {
        let root = TestRoot::new();
        let mut payload = install_payload();
        if mutate == "issuer" {
            payload.issuer = "https://attacker.example".to_string();
        } else {
            payload.audience = "urn:attacker".to_string();
        }
        let result = verifier(&root)
            .verify_install_with_keys(install_request(payload, "2026-08-18T12:00:01Z"), &keys());
        assert!(result.is_err(), "{mutate}");
    }
}

#[test]
fn anonymous_blocked_and_upgrade_outcomes_are_exact() {
    let root = TestRoot::new();
    let mut anonymous = install_payload();
    anonymous.principal = Principal {
        uid: String::new(),
        org: None,
    };
    assert!(verifier(&root)
        .verify_install_with_keys(
            install_request(anonymous.clone(), "2026-08-18T12:00:01Z"),
            &keys(),
        )
        .is_err());
    let mut request = install_request(anonymous, "2026-08-18T12:00:01Z");
    request.expected.owner_uid = None;
    let verified = verifier(&root)
        .verify_install_with_keys(request, &keys())
        .unwrap();
    assert_eq!(verified.status, VerifiedInstallStatus::Active);
    assert_eq!(verified.owner_uid, None);

    let root = TestRoot::new();
    let mut blocked = install_payload();
    blocked.status = "blocked".to_string();
    let verified = verifier(&root)
        .verify_install_with_keys(install_request(blocked, "2026-08-18T12:00:01Z"), &keys())
        .unwrap();
    assert_eq!(verified.status, VerifiedInstallStatus::Blocked);

    let root = TestRoot::new();
    let mut upgrade = install_payload();
    upgrade.min_supported_version = "0.2.0".to_string();
    let verified = verifier(&root)
        .verify_install_with_keys(install_request(upgrade, "2026-08-18T12:00:01Z"), &keys())
        .unwrap();
    assert_eq!(verified.status, VerifiedInstallStatus::UpgradeRequired);

    let root = TestRoot::new();
    let mut nonanonymous = install_request(install_payload(), "2026-08-18T12:00:01Z");
    nonanonymous.expected.owner_uid = None;
    assert!(verifier(&root)
        .verify_install_with_keys(nonanonymous, &keys())
        .is_err());
}

#[test]
fn rejects_rollback_expiry_and_future_issue() {
    let root = TestRoot::new();
    let authority = verifier(&root);
    authority
        .verify_install_with_keys(
            install_request(install_payload(), "2026-08-18T12:00:05Z"),
            &keys(),
        )
        .unwrap();
    authority
        .verify_install_with_keys(
            install_request(install_payload(), "2026-08-18T12:00:05Z"),
            &keys(),
        )
        .expect("an equal server-time observation is idempotent");
    let mut rollback = install_payload();
    rollback.issued_at = "2026-08-18T11:59:00Z".to_string();
    rollback.expires_at = "2026-08-25T11:59:00Z".to_string();
    assert!(authority
        .verify_install_with_keys(install_request(rollback, "2026-08-18T12:00:04Z"), &keys(),)
        .unwrap_err()
        .contains("rolled back"));

    let root = TestRoot::new();
    let mut expired = install_payload();
    expired.expires_at = "2026-08-18T12:00:01Z".to_string();
    assert!(verifier(&root)
        .verify_install_with_keys(install_request(expired, "2026-08-18T12:00:01Z"), &keys(),)
        .is_err());

    let root = TestRoot::new();
    let mut future = install_payload();
    future.issued_at = "2026-08-18T12:00:02Z".to_string();
    assert!(verifier(&root)
        .verify_install_with_keys(install_request(future, "2026-08-18T12:00:01Z"), &keys(),)
        .is_err());
}

#[test]
fn work_grant_is_bound_to_exact_project_engine_scope_and_snapshot() {
    let root = TestRoot::new();
    let verified = verifier(&root)
        .verify_work_with_keys(
            work_request(work_payload(), "2026-08-18T12:00:03Z"),
            &keys(),
        )
        .unwrap();
    assert_eq!(verified.engine_release, RELEASE);
    assert_eq!(verified.snapshot_base_fingerprint, FINGERPRINT);
    assert_eq!(verified.transformer_revision, 7);
    assert_eq!(
        verified.individual_execution.room_content_sha256,
        ROOM_CONTENT_SHA256
    );
    assert_eq!(
        verified
            .individual_execution
            .outputs
            .iter()
            .map(|output| output.format.as_str())
            .collect::<Vec<_>>(),
        ["shp", "kmz", "xlsx"]
    );

    for mutate in [
        "project",
        "engine",
        "scope",
        "snapshot",
        "principal-org",
        "output-order",
    ] {
        let root = TestRoot::new();
        let mut payload = work_payload();
        match mutate {
            "project" => payload.scope.ds_project = "project-2".to_string(),
            "engine" => {
                payload.scope.engines[0].release =
                    "ds-network-reporter@0.1.0+cccccccccccccccccccccccccccccccccccccccc".to_string()
            }
            "scope" => payload.scope.capabilities.push("admin".to_string()),
            "snapshot" => {
                payload
                    .snapshot_base
                    .insert("export-other".to_string(), FINGERPRINT.to_string());
            }
            "principal-org" => payload.principal.org = Some("org-2".to_string()),
            "output-order" => payload.individual_execution.outputs.swap(0, 1),
            _ => unreachable!(),
        }
        assert!(
            verifier(&root)
                .verify_work_with_keys(work_request(payload, "2026-08-18T12:00:03Z"), &keys(),)
                .is_err(),
            "{mutate}"
        );
    }

    let root = TestRoot::new();
    let mut request = work_request(work_payload(), "2026-08-18T12:00:03Z");
    request.snapshot_manifest.transformers[0].revision = 0;
    assert!(verifier(&root)
        .verify_work_with_keys(request, &keys())
        .is_err());

    let root = TestRoot::new();
    let mut request = work_request(work_payload(), "2026-08-18T12:00:03Z");
    request
        .snapshot_manifest
        .individual_execution
        .room_content_sha256 = "d".repeat(64);
    assert!(verifier(&root)
        .verify_work_with_keys(request, &keys())
        .unwrap_err()
        .contains("requested desktop work"));

    let root = TestRoot::new();
    let mut request = work_request(work_payload(), "2026-08-18T12:00:03Z");
    request.capability.individual_execution.outputs.truncate(1);
    assert!(verifier(&root)
        .verify_work_with_keys(request, &keys())
        .unwrap_err()
        .contains("visible capability"));

    let extended = serde_json::json!({
        "schema": 1,
        "engine": { "name": "ds-network-reporter", "release": RELEASE },
        "transformers": [{
            "name": "_001_main",
            "revision": 7,
            "input_base_fingerprint": FINGERPRINT,
            "layers": {},
        }],
        "network_reporter_input": {},
    });
    assert!(serde_json::from_value::<NetworkReporterSnapshotManifest>(extended).is_err());
}

#[test]
fn artifact_read_entitlement_is_short_lived_and_bound_to_exact_batch() {
    let root = TestRoot::new();
    let verified = verifier(&root)
        .verify_artifact_read_with_keys(
            artifact_read_request(artifact_read_payload(), "2026-08-18T12:00:05Z"),
            &keys(),
        )
        .unwrap();
    assert_eq!(verified.owner_uid, OWNER);
    assert_eq!(verified.organization_id.as_deref(), Some("org-1"));
    assert_eq!(verified.ds_project, "project-1");
    assert_eq!(verified.engine_release, RELEASE);
    assert_eq!(verified.grant_id, work_payload().grant_id);
    assert_eq!(verified.batch_id, "d".repeat(64));
    assert_eq!(verified.individual_execution, individual_execution().into());
    let encoded = serde_json::to_value(&verified).unwrap();
    assert_eq!(
        encoded
            .as_object()
            .unwrap()
            .keys()
            .cloned()
            .collect::<HashSet<_>>(),
        HashSet::from([
            "ownerUid".to_string(),
            "organizationId".to_string(),
            "dsProject".to_string(),
            "engineRelease".to_string(),
            "grantId".to_string(),
            "batchId".to_string(),
            "individualExecution".to_string(),
            "issuedAt".to_string(),
            "expiresAt".to_string(),
            "serverTime".to_string(),
        ])
    );

    let root = TestRoot::new();
    let mut historical_release = artifact_read_payload();
    historical_release.scope.engines[0].release =
        format!("ds-network-reporter@0.1.0+{}", "e".repeat(40));
    let verified = verifier(&root)
        .verify_artifact_read_with_keys(
            artifact_read_request(historical_release, "2026-08-18T12:00:05Z"),
            &keys(),
        )
        .expect("a valid signed historical release remains recoverable");
    assert_eq!(
        verified.engine_release,
        format!("ds-network-reporter@0.1.0+{}", "e".repeat(40))
    );

    for mutate in [
        "owner",
        "organization",
        "project",
        "engine-name",
        "release-shape",
        "grant",
        "batch",
        "room",
        "output-order",
    ] {
        let root = TestRoot::new();
        let mut payload = artifact_read_payload();
        match mutate {
            "owner" => payload.principal.uid = "owner-2".to_string(),
            "organization" => payload.principal.org = Some("org-2".to_string()),
            "project" => payload.scope.ds_project = "project-2".to_string(),
            "engine-name" => payload.scope.engines[0].name = "ds-solar".to_string(),
            "release-shape" => payload.scope.engines[0].release = "latest".to_string(),
            "grant" => payload.grant_id = format!("grant_{}", "e".repeat(40)),
            "batch" => payload.batch_id = "e".repeat(64),
            "room" => payload.individual_execution.room_content_sha256 = "e".repeat(64),
            "output-order" => payload.individual_execution.outputs.swap(0, 1),
            _ => unreachable!(),
        }
        let mut request = artifact_read_request(payload, "2026-08-18T12:00:05Z");
        // Project and signed execution are compared to the held publication by
        // the native session gate after cryptographic verification. All other
        // fields also have an exact installation/request expectation here.
        if mutate == "project" || mutate == "room" {
            assert!(verifier(&root)
                .verify_artifact_read_with_keys(request, &keys())
                .is_ok());
        } else {
            if mutate == "grant" {
                request.expected.grant_id = work_payload().grant_id;
            } else if mutate == "batch" {
                request.expected.batch_id = "d".repeat(64);
            }
            assert!(
                verifier(&root)
                    .verify_artifact_read_with_keys(request, &keys())
                    .is_err(),
                "{mutate}"
            );
        }
    }
}

#[test]
fn artifact_read_entitlement_rejects_domain_visibility_schema_and_time_attacks() {
    let root = TestRoot::new();
    let mut wrong_domain = artifact_read_request(artifact_read_payload(), "2026-08-18T12:00:05Z");
    let (signed_payload, signature) = signed_bytes(WORK_DOMAIN, &artifact_read_payload());
    wrong_domain.capability.signed_payload = signed_payload;
    wrong_domain.capability.signature = signature;
    assert!(verifier(&root)
        .verify_artifact_read_with_keys(wrong_domain, &keys())
        .is_err());

    let root = TestRoot::new();
    let mut visible_drift = artifact_read_request(artifact_read_payload(), "2026-08-18T12:00:05Z");
    visible_drift.capability.batch_id = "e".repeat(64);
    assert!(verifier(&root)
        .verify_artifact_read_with_keys(visible_drift, &keys())
        .unwrap_err()
        .contains("visible capability"));

    let root = TestRoot::new();
    let mut overlong = artifact_read_payload();
    overlong.expires_at = "2026-08-18T12:05:05Z".to_string();
    assert!(verifier(&root)
        .verify_artifact_read_with_keys(
            artifact_read_request(overlong, "2026-08-18T12:00:05Z"),
            &keys(),
        )
        .is_err());

    let root = TestRoot::new();
    let mut outlives_lease = artifact_read_request(artifact_read_payload(), "2026-08-18T12:00:05Z");
    outlives_lease.expected.lease_expires_at = "2026-08-18T12:04:00Z".to_string();
    assert!(verifier(&root)
        .verify_artifact_read_with_keys(outlives_lease, &keys())
        .unwrap_err()
        .contains("outlives"));

    let extended = serde_json::json!({
        "schema": 1,
        "issuer": pins().issuer,
        "audience": pins().audience,
        "install_id": INSTALL_ID,
        "device": "windows",
        "principal": { "uid": OWNER, "org": "org-1" },
        "scope": {
            "ds_project": "project-1",
            "capabilities": ["design.report"],
            "engines": [{ "name": "ds-network-reporter", "release": RELEASE }]
        },
        "grant_id": work_payload().grant_id,
        "batch_id": "d".repeat(64),
        "individual_execution": individual_execution(),
        "issued_at": "2026-08-18T12:00:04Z",
        "expires_at": "2026-08-18T12:05:04Z",
        "snapshot_base": { "export-_001_main": FINGERPRINT },
        "signed_payload": "abc",
        "signature": {
            "algorithm": "RS256", "signer": pins().signer,
            "key_id": "test-key", "value": "abc"
        }
    });
    assert!(serde_json::from_value::<ArtifactReadEntitlementCapability>(extended).is_err());
}

struct CountingCertificates {
    calls: Cell<usize>,
    certificates: BTreeMap<String, String>,
}

impl CertificateSource for CountingCertificates {
    fn fetch(&self, _signer: &str) -> Result<BTreeMap<String, String>, String> {
        self.calls.set(self.calls.get() + 1);
        Ok(self.certificates.clone())
    }
}

#[test]
fn certificate_cache_is_used_but_missing_key_forces_bounded_refresh() {
    let root = TestRoot::new();
    let authority = verifier(&root);
    let now = 1_787_059_200;
    let cache = CertificateCache {
        schema: 1,
        contract: CERT_CACHE_CONTRACT.to_string(),
        authority: pins().identity(),
        signer: pins().signer,
        fetched_at_unix: now,
        certificates: BTreeMap::from([(CERT_KEY_ID.to_string(), CERTIFICATE.to_string())]),
    };
    atomic_replace(
        &root.0.join(CERT_CACHE_FILE),
        &encode_record(&cache, MAX_AUTHORITY_RECORD_BYTES).unwrap(),
    )
    .unwrap();
    let source = CountingCertificates {
        calls: Cell::new(0),
        certificates: cache.certificates.clone(),
    };
    authority
        .resolve_public_key(CERT_KEY_ID, &source, now)
        .unwrap();
    assert_eq!(
        source.calls.get(),
        0,
        "fresh matching cache should avoid network"
    );
    assert!(authority
        .resolve_public_key("rotated-key", &source, now)
        .is_err());
    assert_eq!(
        source.calls.get(),
        1,
        "missing key must force exactly one refresh"
    );

    let long_ago = CertificateCache {
        fetched_at_unix: now - 90 * 24 * 60 * 60,
        ..cache
    };
    atomic_replace(
        &root.0.join(CERT_CACHE_FILE),
        &encode_record(&long_ago, MAX_AUTHORITY_RECORD_BYTES).unwrap(),
    )
    .unwrap();
    authority
        .resolve_public_key(CERT_KEY_ID, &source, now)
        .unwrap();
    assert_eq!(
        source.calls.get(),
        1,
        "age of the fetch is not an expiry: the certificate's own window still covers now"
    );
}

/// Failing source: the endpoint is there, but unreachable — the shape of the
/// desktop fault this lane exists to remove.
struct UnreachableCertificates {
    calls: Cell<usize>,
}

impl CertificateSource for UnreachableCertificates {
    fn fetch(&self, _signer: &str) -> Result<BTreeMap<String, String>, String> {
        self.calls.set(self.calls.get() + 1);
        Err("Google signer certificate request failed: timeout: connect".to_string())
    }
}

/// The fixture certificate declares 2026-04-02 .. 2027-04-02, so a cache
/// fetched eleven days before this reading is far outside the hour the old TTL
/// allowed while the certificate itself has months to run.
const CERT_NOT_AFTER_UNIX: i64 = 1_806_685_004;

#[test]
fn retained_certificate_resolves_offline_for_its_issuer_window() {
    let root = TestRoot::new();
    let authority = verifier(&root);
    let now = 1_787_059_200;
    let cache = CertificateCache {
        schema: 1,
        contract: CERT_CACHE_CONTRACT.to_string(),
        authority: pins().identity(),
        signer: pins().signer,
        fetched_at_unix: now - 11 * 24 * 60 * 60,
        certificates: BTreeMap::from([(CERT_KEY_ID.to_string(), CERTIFICATE.to_string())]),
    };
    atomic_replace(
        &root.0.join(CERT_CACHE_FILE),
        &encode_record(&cache, MAX_AUTHORITY_RECORD_BYTES).unwrap(),
    )
    .unwrap();

    let source = UnreachableCertificates {
        calls: Cell::new(0),
    };
    let key = authority
        .resolve_public_key(CERT_KEY_ID, &source, now)
        .expect("a certificate inside its own validity window resolves with no endpoint at all");
    assert_eq!(
        key,
        parse_rsa_certificate(CERTIFICATE).unwrap(),
        "the retained certificate's key is the one served"
    );
    assert_eq!(
        source.calls.get(),
        0,
        "reaching the certificate endpoint must not be a precondition"
    );
}

#[test]
fn expired_certificate_offline_fails_naming_its_own_window() {
    let root = TestRoot::new();
    let authority = verifier(&root);
    let after_expiry = CERT_NOT_AFTER_UNIX + 60;
    let cache = CertificateCache {
        schema: 1,
        contract: CERT_CACHE_CONTRACT.to_string(),
        authority: pins().identity(),
        signer: pins().signer,
        fetched_at_unix: 1_787_059_200,
        certificates: BTreeMap::from([(CERT_KEY_ID.to_string(), CERTIFICATE.to_string())]),
    };
    atomic_replace(
        &root.0.join(CERT_CACHE_FILE),
        &encode_record(&cache, MAX_AUTHORITY_RECORD_BYTES).unwrap(),
    )
    .unwrap();

    let source = UnreachableCertificates {
        calls: Cell::new(0),
    };
    let error = authority
        .resolve_public_key(CERT_KEY_ID, &source, after_expiry)
        .expect_err("a certificate past its own notAfter is not trusted because it is on disk");
    assert_eq!(
        source.calls.get(),
        1,
        "an expired certificate must still force exactly one bounded refresh"
    );
    assert!(
        error.contains("left its own validity window")
            && error.contains(&CERT_NOT_AFTER_UNIX.to_string())
            && error.contains("timeout: connect"),
        "the refusal must say which fact refused it, and why the refresh failed: {error}"
    );
}

#[test]
fn freshly_fetched_certificate_outside_its_window_is_refused() {
    let root = TestRoot::new();
    let authority = verifier(&root);
    let source = CountingCertificates {
        calls: Cell::new(0),
        certificates: BTreeMap::from([(CERT_KEY_ID.to_string(), CERTIFICATE.to_string())]),
    };
    let error = authority
        .resolve_public_key(CERT_KEY_ID, &source, CERT_NOT_AFTER_UNIX + 60)
        .expect_err("the issuer's window governs a fetched certificate exactly as a retained one");
    assert!(
        error.contains("valid only from unix") && error.contains("clock reading"),
        "unexpected refusal: {error}"
    );
}

#[cfg(unix)]
#[test]
fn high_water_and_cache_refuse_symbolic_links() {
    use std::os::unix::fs::symlink;

    let root = TestRoot::new();
    let target = root.0.join("target");
    std::fs::write(&target, b"{}\n").unwrap();
    symlink(&target, root.0.join(HIGH_WATER_FILE)).unwrap();
    let result = verifier(&root).verify_install_with_keys(
        install_request(install_payload(), "2026-08-18T12:00:01Z"),
        &keys(),
    );
    assert!(result.is_err());

    let root = TestRoot::new();
    let target = root.0.join("certificate-target");
    std::fs::write(&target, b"{}\n").unwrap();
    symlink(&target, root.0.join(CERT_CACHE_FILE)).unwrap();
    let source = CountingCertificates {
        calls: Cell::new(0),
        certificates: BTreeMap::from([(CERT_KEY_ID.to_string(), CERTIFICATE.to_string())]),
    };
    assert!(verifier(&root)
        .resolve_public_key(CERT_KEY_ID, &source, 1_787_059_200)
        .is_err());
}

#[cfg(unix)]
#[test]
fn high_water_lock_serializes_processes_without_stale_age_breaking() {
    let root = TestRoot::new();
    let held = acquire_authority_lock_with_attempts(&root.0, 1).unwrap();
    assert!(acquire_authority_lock_with_attempts(&root.0, 1)
        .unwrap_err()
        .contains("busy"));
    drop(held);
    acquire_authority_lock_with_attempts(&root.0, 1).unwrap();
}
