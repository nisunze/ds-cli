//! Exact-byte, two-lane native client catalog discovery.

use std::fs;
use std::path::{Path, PathBuf};

use ds_cli_contract::Failure;
use ds_cli_contract::spec::Availability;
use ds_client_core::{CLIENT_PROFILE_SCHEMA, ClientProfile, ClientProfileInput, DeploymentLane};
use serde::Deserialize;
use sha2::{Digest, Sha256};

const MAX_CATALOG_BYTES: u64 = 64 * 1024;
const CATALOG_RELATIVE: &str = "ds-client-profiles/catalog.json";
const PINNED_DIGEST: Option<&str> = option_env!("DS_NATIVE_CLIENT_PROFILE_SHA256");
const PRODUCT_ROOT: Option<&str> = option_env!("DS_NATIVE_CLIENT_PRODUCT_ROOT");
#[cfg(debug_assertions)]
const DEV_BUNDLE_ENV: &str = "DS_NATIVE_CLIENT_PROFILE_BUNDLE";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lane {
    Stable,
    Canary,
}

impl Lane {
    pub fn parse(value: &str) -> Result<Self, Failure> {
        match value {
            "stable" => Ok(Self::Stable),
            "canary" => Ok(Self::Canary),
            _ => Err(
                Failure::invalid("invalid_lane", "lane must be stable or canary")
                    .remedy("pass --lane stable or --lane canary"),
            ),
        }
    }

    pub const fn token(self) -> &'static str {
        match self {
            Self::Stable => "stable",
            Self::Canary => "canary",
        }
    }

    const fn core(self) -> DeploymentLane {
        match self {
            Self::Stable => DeploymentLane::Stable,
            Self::Canary => DeploymentLane::Canary,
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Catalog {
    schema_version: String,
    development: bool,
    profiles: Profiles,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Profiles {
    stable: Entry,
    canary: Entry,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Entry {
    firebase: Firebase,
    gateway: Gateway,
    projects_read: ProjectsRead,
    transformer_context: TransformerContext,
    project_forms: ProjectForms,
    provenance: Provenance,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Firebase {
    project_id: String,
    api_key: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Gateway {
    origin: String,
    api_key: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ProjectsRead {
    method: String,
    path: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TransformerContext {
    method: String,
    path: String,
    action: String,
    fields: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ProjectForms {
    method: String,
    path: String,
    action: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Provenance {
    source_revision: String,
    descriptor_sha256: String,
}

pub fn load(lane: Lane) -> Result<ClientProfile, Failure> {
    let (path, development, expected) = discovery()?;
    load_path(&path, lane, development, expected)
}

pub fn availability() -> Availability {
    match (load(Lane::Stable), load(Lane::Canary)) {
        (Ok(_), Ok(_)) => Availability::Available,
        _ => Availability::unavailable(
            "native_profile_not_configured",
            "this ds build has no valid digest-pinned two-lane native client catalog",
            "install a ds release that includes ds-client-profiles/catalog.json",
        ),
    }
}

fn discovery() -> Result<(PathBuf, bool, String), Failure> {
    #[cfg(debug_assertions)]
    if let Some(path) = std::env::var_os(DEV_BUNDLE_ENV).filter(|value| !value.is_empty()) {
        return Ok((PathBuf::from(path), true, String::new()));
    }

    let expected = PINNED_DIGEST
        .filter(|value| valid_digest(value))
        .ok_or_else(not_configured)?
        .to_owned();
    #[cfg(windows)]
    let path = std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(|parent| parent.join(CATALOG_RELATIVE)))
        .ok_or_else(not_configured)?;
    #[cfg(not(windows))]
    let path = PRODUCT_ROOT
        .filter(|root| {
            *root == "/usr/lib/DS GridDesign" || *root == "/usr/lib/DS GridDesign Canary"
        })
        .map(|root| PathBuf::from(root).join(CATALOG_RELATIVE))
        .ok_or_else(not_configured)?;
    if !path.is_file() {
        return Err(not_configured());
    }
    Ok((path, false, expected))
}

fn load_path(
    path: &Path,
    lane: Lane,
    allow_development: bool,
    expected: String,
) -> Result<ClientProfile, Failure> {
    let metadata = fs::symlink_metadata(path).map_err(|_| not_configured())?;
    if metadata.file_type().is_symlink()
        || !metadata.is_file()
        || metadata.len() > MAX_CATALOG_BYTES
    {
        return Err(unsafe_catalog());
    }
    let bytes = fs::read(path).map_err(|_| unsafe_catalog())?;
    if bytes.len() as u64 > MAX_CATALOG_BYTES {
        return Err(unsafe_catalog());
    }
    let actual = format!("{:x}", Sha256::digest(&bytes));
    if !allow_development && actual != expected {
        return Err(Failure::unavailable(
            "native_profile_digest_mismatch",
            "the packaged native client catalog does not match this ds build",
        )
        .remedy("reinstall ds from one complete signed release"));
    }
    let catalog: Catalog = serde_json::from_slice(&bytes).map_err(|_| unsafe_catalog())?;
    if catalog.schema_version != CLIENT_PROFILE_SCHEMA || catalog.development != allow_development {
        return Err(unsafe_catalog());
    }
    let entry = match lane {
        Lane::Stable => catalog.profiles.stable,
        Lane::Canary => catalog.profiles.canary,
    };
    ClientProfile::validate(ClientProfileInput {
        schema_version: catalog.schema_version,
        lane: lane.core(),
        source_revision: entry.provenance.source_revision,
        descriptor_sha256: entry.provenance.descriptor_sha256,
        firebase_project_id: entry.firebase.project_id,
        firebase_api_key: entry.firebase.api_key,
        gateway_api_key: entry.gateway.api_key,
        gateway_origin: entry.gateway.origin,
        project_list_method: entry.projects_read.method,
        project_list_path: entry.projects_read.path,
        transformer_context_method: entry.transformer_context.method,
        transformer_context_path: entry.transformer_context.path,
        transformer_context_action: entry.transformer_context.action,
        transformer_context_fields: entry.transformer_context.fields,
        project_forms_method: entry.project_forms.method,
        project_forms_path: entry.project_forms.path,
        project_forms_action: entry.project_forms.action,
    })
    .map_err(|_| unsafe_catalog())
}

fn valid_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn not_configured() -> Failure {
    Failure::unavailable(
        "native_profile_not_configured",
        "this ds build has no complete packaged native client catalog",
    )
    .remedy("install a ds release that includes its digest-pinned native client profiles")
}

fn unsafe_catalog() -> Failure {
    Failure::unavailable(
        "native_profile_unsafe",
        "the native client catalog is missing, unsafe, oversized, or malformed",
    )
    .remedy("reinstall ds from one complete signed release")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::sync::atomic::{AtomicU64, Ordering};

    static SEQUENCE: AtomicU64 = AtomicU64::new(1);

    fn fixture(development: bool) -> Vec<u8> {
        serde_json::to_vec(&json!({
            "schema_version": CLIENT_PROFILE_SCHEMA,
            "development": development,
            "profiles": {
                "stable": {
                    "firebase": { "project_id": "stable-project", "api_key": "firebase-public" },
                    "gateway": { "origin": "https://stable.ue.gateway.dev", "api_key": "gateway-public" },
                    "projects_read": { "method": "GET", "path": "/api/v1/user/projects" },
                    "transformer_context": { "method": "POST", "path": "/api/v1/data", "action": "get_transformers_data", "fields": "context" },
                    "project_forms": { "method": "POST", "path": "/api/v1/project-forms", "action": "activate" },
                    "provenance": { "source_revision": "abc123", "descriptor_sha256": "a".repeat(64) }
                },
                "canary": {
                    "firebase": { "project_id": "canary-project", "api_key": "firebase-canary" },
                    "gateway": { "origin": "https://ds-canary.ue.gateway.dev", "api_key": "gateway-canary" },
                    "projects_read": { "method": "GET", "path": "/api/v1/user/projects" },
                    "transformer_context": { "method": "POST", "path": "/api/v1/data", "action": "get_transformers_data", "fields": "context" },
                    "project_forms": { "method": "POST", "path": "/api/v1/project-forms", "action": "activate" },
                    "provenance": { "source_revision": "def456", "descriptor_sha256": "b".repeat(64) }
                }
            }
        }))
        .unwrap()
    }

    fn with_fixture(bytes: &[u8], test: impl FnOnce(&Path)) {
        let path = std::env::temp_dir().join(format!(
            "ds-auth-profile-{}-{}.json",
            std::process::id(),
            SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        fs::write(&path, bytes).unwrap();
        test(&path);
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn release_without_compile_pin_is_typed_not_configured() {
        if PINNED_DIGEST.is_none() {
            assert_eq!(not_configured().code(), "native_profile_not_configured");
        }
    }

    #[test]
    fn lanes_are_closed_and_stable_is_explicit() {
        assert_eq!(Lane::parse("stable").unwrap(), Lane::Stable);
        assert_eq!(Lane::parse("canary").unwrap(), Lane::Canary);
        assert!(Lane::parse("prod").is_err());
    }

    #[test]
    fn exact_digest_and_two_nested_lanes_are_required() {
        let bytes = fixture(false);
        let digest = format!("{:x}", Sha256::digest(&bytes));
        with_fixture(&bytes, |path| {
            let profile = load_path(path, Lane::Stable, false, digest.clone()).unwrap();
            assert_eq!(profile.lane(), DeploymentLane::Stable);
            assert_eq!(profile.source_revision(), "abc123");
            assert_eq!(
                load_path(path, Lane::Stable, false, "0".repeat(64))
                    .unwrap_err()
                    .code(),
                "native_profile_digest_mismatch"
            );
        });
    }

    #[test]
    fn development_marker_cannot_cross_release_or_debug_gate() {
        let release = fixture(false);
        let development = fixture(true);
        with_fixture(&release, |path| {
            assert_eq!(
                load_path(path, Lane::Stable, true, String::new())
                    .unwrap_err()
                    .code(),
                "native_profile_unsafe"
            );
        });
        with_fixture(&development, |path| {
            assert_eq!(
                load_path(
                    path,
                    Lane::Stable,
                    false,
                    format!("{:x}", Sha256::digest(&development))
                )
                .unwrap_err()
                .code(),
                "native_profile_unsafe"
            );
            assert!(load_path(path, Lane::Canary, true, String::new()).is_ok());
        });
    }

    #[test]
    fn missing_lane_and_oversized_catalog_are_refused() {
        let mut missing: serde_json::Value = serde_json::from_slice(&fixture(false)).unwrap();
        missing["profiles"]
            .as_object_mut()
            .unwrap()
            .remove("canary");
        let missing = serde_json::to_vec(&missing).unwrap();
        with_fixture(&missing, |path| {
            assert_eq!(
                load_path(
                    path,
                    Lane::Stable,
                    false,
                    format!("{:x}", Sha256::digest(&missing))
                )
                .unwrap_err()
                .code(),
                "native_profile_unsafe"
            );
        });

        let oversized = vec![b'x'; MAX_CATALOG_BYTES as usize + 1];
        with_fixture(&oversized, |path| {
            assert_eq!(
                load_path(path, Lane::Stable, false, "0".repeat(64))
                    .unwrap_err()
                    .code(),
                "native_profile_unsafe"
            );
        });
    }

    #[test]
    fn v3_fixed_read_blocks_are_required_and_exact() {
        let mut missing: serde_json::Value = serde_json::from_slice(&fixture(false)).unwrap();
        missing["profiles"]["stable"]
            .as_object_mut()
            .unwrap()
            .remove("transformer_context");
        let missing = serde_json::to_vec(&missing).unwrap();
        with_fixture(&missing, |path| {
            assert_eq!(
                load_path(
                    path,
                    Lane::Stable,
                    false,
                    format!("{:x}", Sha256::digest(&missing))
                )
                .unwrap_err()
                .code(),
                "native_profile_unsafe"
            );
        });

        let mut escaped: serde_json::Value = serde_json::from_slice(&fixture(false)).unwrap();
        escaped["profiles"]["stable"]["transformer_context"]["path"] = json!("/api/v1/anything");
        let escaped = serde_json::to_vec(&escaped).unwrap();
        with_fixture(&escaped, |path| {
            assert_eq!(
                load_path(
                    path,
                    Lane::Stable,
                    false,
                    format!("{:x}", Sha256::digest(&escaped))
                )
                .unwrap_err()
                .code(),
                "native_profile_unsafe"
            );
        });

        let mut escaped: serde_json::Value = serde_json::from_slice(&fixture(false)).unwrap();
        escaped["profiles"]["stable"]["project_forms"]["action"] = json!("bulk_save");
        let escaped = serde_json::to_vec(&escaped).unwrap();
        with_fixture(&escaped, |path| {
            assert_eq!(
                load_path(
                    path,
                    Lane::Stable,
                    false,
                    format!("{:x}", Sha256::digest(&escaped))
                )
                .unwrap_err()
                .code(),
                "native_profile_unsafe"
            );
        });
    }

    #[cfg(unix)]
    #[test]
    fn catalog_symlink_is_refused() {
        use std::os::unix::fs::symlink;

        let target = std::env::temp_dir().join(format!(
            "ds-auth-profile-target-{}-{}.json",
            std::process::id(),
            SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        let link = target.with_extension("link");
        let bytes = fixture(false);
        fs::write(&target, &bytes).unwrap();
        symlink(&target, &link).unwrap();
        assert_eq!(
            load_path(
                &link,
                Lane::Stable,
                false,
                format!("{:x}", Sha256::digest(&bytes))
            )
            .unwrap_err()
            .code(),
            "native_profile_unsafe"
        );
        fs::remove_file(link).unwrap();
        fs::remove_file(target).unwrap();
    }
}
