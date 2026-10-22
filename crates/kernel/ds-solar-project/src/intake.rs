//! Engine-free admission of one authenticated, bounded Solar snapshot.
//! The host owns authentication and acquisition. This owner preserves the exact
//! snapshot fingerprint and drops temporary media URL authority before writing.
use anyhow::{Context, Result, bail, ensure};
use ds_solar_contracts::{
    CityIdentity, RunOptions, SnapshotRevision, SolarCityInput, SystemType,
    is_solar_id as canonical_compute_id, version::GOVERNED_CITY_INTAKE_SCHEMA_VERSION,
};
use serde::{Deserialize, Serialize};
use std::{io::Write, path::Path};

pub const MAX_GOVERNED_SNAPSHOT_RESPONSE_BYTES: usize = 32 * 1024 * 1024;
pub const MAX_GOVERNED_SNAPSHOT_BYTES: usize = 8 * 1024 * 1024;
const MAX_GOVERNED_INTAKE_BYTES: usize = 32 * 1024 * 1024;

/// A closed capture request. Explicit identity fences the authenticated response
/// before any output side effect; this value grants no publication authority.
pub struct CaptureRequest<'a> {
    pub project_id: &'a str,
    pub city_id: &'a str,
    pub snapshot_bytes: &'a [u8],
}

pub fn capture(request: CaptureRequest<'_>, out: &Path) -> Result<GovernedCityIntake> {
    ensure!(
        canonical_compute_id(request.project_id) && canonical_compute_id(request.city_id),
        "governed Solar capture project or city id is not canonical"
    );
    ensure!(
        !request.snapshot_bytes.is_empty()
            && request.snapshot_bytes.len() <= MAX_GOVERNED_SNAPSHOT_RESPONSE_BYTES,
        "governed Solar snapshot response exceeds its byte boundary"
    );
    let snapshot: GovernedSnapshot = serde_json::from_slice(request.snapshot_bytes)
        .context("governed Solar snapshot response is invalid")?;
    let intake = governed_intake(snapshot)?;
    ensure!(
        intake.city_input.identity.project_id == request.project_id
            && intake.city_input.identity.city_id == request.city_id,
        "governed Solar snapshot disagrees with the captured project or city"
    );
    let mut bytes = serde_json::to_vec_pretty(&intake)?;
    bytes.push(b'\n');
    ensure!(
        bytes.len() <= MAX_GOVERNED_INTAKE_BYTES,
        "governed Solar intake exceeds its byte boundary"
    );
    // The destination names only a complete synced private inode, and can never
    // replace an existing file (including a raced path or dangling symlink).
    let parent = out
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    std::fs::create_dir_all(parent)?;
    let mut stage = tempfile::NamedTempFile::new_in(parent)?;
    stage.write_all(&bytes)?;
    stage.as_file().sync_all()?;
    stage.persist_noclobber(out).map_err(|e| e.error)?;
    let _ = crate::files::sync_dir(parent);
    Ok(intake)
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GovernedSnapshot {
    pub snapshot_json: String,
    pub snapshot_sha256: String,
    pub input_base_fingerprint: String,
    pub media_downloads: Vec<serde_json::Value>,
    pub snapshot_bytes: usize,
    pub source_document_count: usize,
    pub firestore_read_time: String,
    pub snapshot_receipt_id: String,
    pub snapshot_receipt_expires_at: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GovernedIntakeAuthority {
    pub source_snapshot_sha256: String,
    pub input_base_fingerprint: String,
    pub snapshot_receipt_id: String,
    pub snapshot_receipt_expires_at: String,
    pub firestore_read_time: String,
    pub snapshot_bytes: usize,
    pub source_document_count: usize,
    pub media_download_count: usize,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GovernedCityIntake {
    pub schema_version: String,
    pub city_input: SolarCityInput,
    pub authority: GovernedIntakeAuthority,
}

pub fn governed_intake(governed: GovernedSnapshot) -> Result<GovernedCityIntake> {
    if governed.snapshot_json.is_empty()
        || governed.snapshot_bytes != governed.snapshot_json.len()
        || governed.snapshot_bytes > MAX_GOVERNED_SNAPSHOT_BYTES
    {
        bail!("governed Solar snapshot byte declaration is invalid");
    }
    if governed.source_document_count == 0
        || governed.source_document_count > 512
        || governed.media_downloads.len() > 512
    {
        bail!("governed Solar snapshot exceeds its document or media boundary");
    }
    if !lower_sha256(&governed.snapshot_sha256) || !lower_sha256(&governed.input_base_fingerprint) {
        bail!("governed Solar snapshot digest declaration is invalid");
    }
    let actual_snapshot_sha256 =
        ds_solar_contracts::digest::hex_sha256(governed.snapshot_json.as_bytes());
    if actual_snapshot_sha256 != governed.snapshot_sha256 {
        bail!("governed Solar snapshot bytes do not match snapshot_sha256");
    }
    if !governed
        .media_downloads
        .iter()
        .all(serde_json::Value::is_object)
    {
        bail!("governed Solar media download authority is not an object list");
    }
    if !canonical_uuid(&governed.snapshot_receipt_id) {
        bail!("governed Solar snapshot receipt id is invalid");
    }
    let read_time = chrono::DateTime::parse_from_rfc3339(&governed.firestore_read_time)
        .context("governed Solar Firestore read time is invalid")?;
    let receipt_expiry =
        chrono::DateTime::parse_from_rfc3339(&governed.snapshot_receipt_expires_at)
            .context("governed Solar snapshot receipt expiry is invalid")?;
    if receipt_expiry <= chrono::Utc::now() {
        bail!("governed Solar snapshot receipt has expired");
    }

    let snapshot: serde_json::Value = serde_json::from_str(&governed.snapshot_json)
        .context("governed Solar snapshot_json is invalid")?;
    let snapshot_object = snapshot
        .as_object()
        .context("governed Solar snapshot_json is not an object")?;
    let mut snapshot_keys = snapshot_object
        .keys()
        .map(String::as_str)
        .collect::<Vec<_>>();
    snapshot_keys.sort_unstable();
    if snapshot_keys
        != [
            "00_templates",
            "01_city_inputs",
            "_config",
            "_provenance",
            "_root",
        ]
    {
        bail!("governed Solar snapshot has an unexpected top-level section");
    }
    let provenance = snapshot
        .get("_provenance")
        .and_then(serde_json::Value::as_object)
        .context("governed Solar snapshot provenance is missing")?;
    if provenance.get("schema").and_then(serde_json::Value::as_u64) != Some(3)
        || provenance
            .get("source_kind")
            .and_then(serde_json::Value::as_str)
            != Some("authenticated ds-brain desktop snapshot")
    {
        bail!("governed Solar snapshot provenance is not ds-brain schema 3");
    }
    let root = trimmed_control_free_text(provenance.get("root"), "_provenance.root")?;
    let city_id = trimmed_control_free_text(provenance.get("context"), "_provenance.context")?;
    if provenance
        .get("input_base_fingerprint")
        .and_then(serde_json::Value::as_str)
        != Some(governed.input_base_fingerprint.as_str())
    {
        bail!("governed Solar snapshot input fingerprint disagrees with its provenance");
    }
    // ds-brain hashes Go JSON bytes. Preserve the already SHA-verified raw
    // section bytes: parsing/re-encoding changes float notation and HTML escapes.
    // The server emits sorted keys, so removing provenance recovers its exact
    // four-section read set without guessing a cross-language number formatter.
    let mut raw_sections: std::collections::BTreeMap<String, Box<serde_json::value::RawValue>> =
        serde_json::from_str(&governed.snapshot_json)?;
    raw_sections.remove("_provenance");
    let recomputed_fingerprint =
        ds_solar_contracts::digest::hex_sha256(&serde_json::to_vec(&raw_sections)?);
    if recomputed_fingerprint != governed.input_base_fingerprint {
        bail!("governed Solar snapshot input fingerprint does not match its read set");
    }
    let expected_seed = ds_solar_contracts::digest::demand_noise_seed(city_id)?;
    if provenance
        .get("demand_noise_seed")
        .and_then(serde_json::Value::as_u64)
        != Some(u64::from(expected_seed))
    {
        bail!("governed Solar snapshot demand seed is not canonical");
    }
    let project_id = project_from_root(root)?;
    if !canonical_compute_id(project_id) || !canonical_compute_id(city_id) {
        bail!("governed Solar snapshot project or city id is not canonical");
    }
    let display_name = trimmed_control_free_text(
        snapshot
            .get("01_city_inputs")
            .and_then(|value| value.get("city"))
            .and_then(|value| value.get("city")),
        "01_city_inputs.city.city",
    )?;

    let city_input = SolarCityInput::from_city_snapshot(
        CityIdentity {
            project_id: project_id.to_string(),
            root: root.to_string(),
            city_id: city_id.to_string(),
            display_name: display_name.to_string(),
        },
        SnapshotRevision {
            source: "ds-brain".to_string(),
            captured_at: read_time.to_rfc3339(),
            record_updated_at_ms: None,
            revision: Some(governed.input_base_fingerprint.clone()),
        },
        RunOptions {
            system_types: vec![
                SystemType::Hybrid,
                SystemType::SolarBattery,
                SystemType::ThermalOnly,
            ],
            language: "fr".to_string(),
            demand_noise_seed: Some(expected_seed),
        },
        &snapshot,
    )?;

    Ok(GovernedCityIntake {
        schema_version: GOVERNED_CITY_INTAKE_SCHEMA_VERSION.to_string(),
        city_input,
        authority: GovernedIntakeAuthority {
            source_snapshot_sha256: governed.snapshot_sha256,
            input_base_fingerprint: governed.input_base_fingerprint,
            snapshot_receipt_id: governed.snapshot_receipt_id,
            snapshot_receipt_expires_at: receipt_expiry.to_rfc3339(),
            firestore_read_time: read_time.to_rfc3339(),
            snapshot_bytes: governed.snapshot_bytes,
            source_document_count: governed.source_document_count,
            media_download_count: governed.media_downloads.len(),
        },
    })
}

pub fn lower_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

pub fn canonical_uuid(value: &str) -> bool {
    value.len() == 36
        && value.bytes().enumerate().all(|(index, byte)| match index {
            8 | 13 | 18 | 23 => byte == b'-',
            _ => byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte),
        })
        && matches!(value.as_bytes()[14], b'1'..=b'5')
        && matches!(value.as_bytes()[19], b'8' | b'9' | b'a' | b'b')
}

fn trimmed_control_free_text<'a>(
    value: Option<&'a serde_json::Value>,
    field: &str,
) -> Result<&'a str> {
    let text = value
        .and_then(serde_json::Value::as_str)
        .context(format!("governed Solar snapshot {field} is missing"))?;
    if text.is_empty() || text.trim() != text || text.chars().any(char::is_control) {
        bail!("governed Solar snapshot {field} is invalid");
    }
    Ok(text)
}

pub fn project_from_root(root: &str) -> Result<&str> {
    let mut parts = root.split('/');
    match (parts.next(), parts.next(), parts.next(), parts.next()) {
        (Some("eds_project"), Some(project), Some("eds_solar"), None) => Ok(project),
        _ => bail!("governed Solar snapshot root is not project-scoped"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn governed_snapshot_fixture() -> GovernedSnapshot {
        let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../../../ds-solar/fixtures/city/aderm_bere.snapshot.json");
        let mut snapshot: serde_json::Value =
            serde_json::from_slice(&std::fs::read(path).expect("fixture snapshot"))
                .expect("fixture json");
        let city_id = "aderm_bere";
        snapshot["_config"] = serde_json::json!({
            "paths": { "root": "eds_project/project-1/eds_solar/aderm_bere" }
        });
        let input_base = serde_json::json!({
            "_root": snapshot.get("_root").unwrap(),
            "_config": snapshot.get("_config").unwrap(),
            "00_templates": snapshot.get("00_templates").unwrap(),
            "01_city_inputs": snapshot.get("01_city_inputs").unwrap(),
        });
        let fingerprint = ds_solar_contracts::digest::hex_sha256(
            &ds_solar_contracts::digest::canonical_json_bytes(&input_base).unwrap(),
        );
        snapshot["_provenance"] = serde_json::json!({
            "schema": 3,
            "source_kind": "authenticated ds-brain desktop snapshot",
            "root": "eds_project/project-1/eds_solar",
            "context": city_id,
            "demand_noise_seed": ds_solar_contracts::digest::demand_noise_seed(city_id).unwrap(),
            "input_base_fingerprint": fingerprint,
        });
        let snapshot_json =
            String::from_utf8(ds_solar_contracts::digest::canonical_json_bytes(&snapshot).unwrap())
                .unwrap();
        GovernedSnapshot {
            snapshot_sha256: ds_solar_contracts::digest::hex_sha256(snapshot_json.as_bytes()),
            snapshot_bytes: snapshot_json.len(),
            snapshot_json,
            input_base_fingerprint: fingerprint,
            media_downloads: vec![],
            source_document_count: 12,
            firestore_read_time: "2026-08-30T00:00:00Z".to_string(),
            snapshot_receipt_id: "21c655a1-1037-52ce-a601-e9c7e3734d53".to_string(),
            snapshot_receipt_expires_at: "2099-08-30T00:00:00Z".to_string(),
        }
    }

    fn capture_bytes(
        bytes: &[u8],
        out: &Path,
        project: &str,
        city: &str,
    ) -> Result<GovernedCityIntake> {
        capture(
            CaptureRequest {
                project_id: project,
                city_id: city,
                snapshot_bytes: bytes,
            },
            out,
        )
    }

    #[test]
    fn real_governed_fixture_preserves_identity_revision_and_digests_without_media_urls() {
        let mut snapshot = governed_snapshot_fixture();
        snapshot
            .media_downloads
            .push(serde_json::json!({"download_url": "https://signed.example/secret"}));
        let digest = snapshot.snapshot_sha256.clone();
        let fingerprint = snapshot.input_base_fingerprint.clone();
        let expected = governed_intake(governed_snapshot_fixture()).unwrap();
        let bytes = serde_json::to_vec(&snapshot).unwrap();
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("capture.json");
        let intake = capture_bytes(&bytes, &path, "project-1", "aderm_bere").unwrap();
        assert_eq!(
            intake.city_input.identity.root,
            "eds_project/project-1/eds_solar"
        );
        assert_eq!(
            intake.city_input.content_digest,
            expected.city_input.content_digest
        );
        assert_eq!(
            intake.city_input.revision.revision.as_deref(),
            Some(fingerprint.as_str())
        );
        assert_eq!(intake.authority.source_snapshot_sha256, digest);
        assert_eq!(intake.authority.media_download_count, 1);
        intake.city_input.validate().unwrap();
        let stored = std::fs::read(&path).unwrap();
        assert!(!String::from_utf8_lossy(&stored).contains("signed.example"));
        let decoded: GovernedCityIntake = serde_json::from_slice(&stored).unwrap();
        assert_eq!(
            decoded.city_input.content_digest,
            intake.city_input.content_digest
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
    }

    #[test]
    fn malformed_unknown_cross_project_and_cross_city_refuse_before_effects() {
        let snapshot = serde_json::to_vec(&governed_snapshot_fixture()).unwrap();
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("absent/capture.json");
        for (bytes, project, city) in [
            (b"{}".as_slice(), "project-1", "aderm_bere"),
            (b"not-json".as_slice(), "project-1", "aderm_bere"),
            (snapshot.as_slice(), "project-other", "aderm_bere"),
            (snapshot.as_slice(), "project-1", "other_city"),
        ] {
            assert!(capture_bytes(bytes, &path, project, city).is_err());
            assert!(!path.parent().unwrap().exists());
        }
        let mut unknown = serde_json::to_value(governed_snapshot_fixture()).unwrap();
        unknown["unexpected"] = serde_json::json!(true);
        assert!(
            capture_bytes(
                &serde_json::to_vec(&unknown).unwrap(),
                &path,
                "project-1",
                "aderm_bere"
            )
            .is_err()
        );
        assert!(!path.parent().unwrap().exists());
    }

    #[test]
    fn digest_tampering_receipt_expiry_and_bounds_are_refused() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("capture.json");
        let mut tampered = governed_snapshot_fixture();
        tampered.snapshot_sha256 = "a".repeat(64);
        assert!(
            capture_bytes(
                &serde_json::to_vec(&tampered).unwrap(),
                &path,
                "project-1",
                "aderm_bere"
            )
            .is_err()
        );
        let mut expired = governed_snapshot_fixture();
        expired.snapshot_receipt_expires_at = "2000-01-01T00:00:00Z".into();
        assert!(
            capture_bytes(
                &serde_json::to_vec(&expired).unwrap(),
                &path,
                "project-1",
                "aderm_bere"
            )
            .is_err()
        );
        let mut too_many = governed_snapshot_fixture();
        too_many.source_document_count = 513;
        assert!(governed_intake(too_many).is_err());
        let oversized = vec![b' '; MAX_GOVERNED_SNAPSHOT_RESPONSE_BYTES + 1];
        assert!(
            capture_bytes(&oversized, &path, "project-1", "aderm_bere")
                .unwrap_err()
                .to_string()
                .contains("byte boundary")
        );
        assert!(!path.exists());
    }

    #[test]
    fn existing_output_and_dangling_link_are_never_replaced() {
        let bytes = serde_json::to_vec(&governed_snapshot_fixture()).unwrap();
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("capture.json");
        std::fs::write(&path, b"retained").unwrap();
        assert!(capture_bytes(&bytes, &path, "project-1", "aderm_bere").is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"retained");
        #[cfg(unix)]
        {
            std::fs::remove_file(&path).unwrap();
            let missing = root.path().join("missing");
            std::os::unix::fs::symlink(&missing, &path).unwrap();
            assert!(capture_bytes(&bytes, &path, "project-1", "aderm_bere").is_err());
            assert!(!missing.exists());
            assert!(
                std::fs::symlink_metadata(path)
                    .unwrap()
                    .file_type()
                    .is_symlink()
            );
        }
        assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 1);
    }
}
