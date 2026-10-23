//! Cache-only preparation and prepared-input admission shared by local adapters.
//! This owner has no provider parameter, executable selection or calculation runtime.
use crate::intake::{
    GovernedCityIntake, GovernedIntakeAuthority, MAX_GOVERNED_SNAPSHOT_BYTES, canonical_uuid,
    lower_sha256, project_from_root,
};
use anyhow::{Context, Result, bail};
use ds_solar_contracts::{PreparedSolarCityInput, version::GOVERNED_CITY_INTAKE_SCHEMA_VERSION};
use ds_solar_host::cache::DirectoryReferenceCache;
use ds_solar_reference_host::ReferencePreparer;
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
};

/// The existing local prepared-input read bound, applied to every discovered file.
pub const MAX_PREPARED_INPUT_BYTES: u64 = 64 * 1024 * 1024;

/// A local handoff claim, never proof that publication is authorized.
///
/// ds-brain revalidates the actor-bound receipt, current project fingerprint,
/// build admission and project access when `/api/v1/compute_artifacts` opens.
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PreparedPublicationClaim {
    pub schema_version: String,
    pub claim_kind: String,
    pub project_id: String,
    pub city_id: String,
    pub city_content_digest: String,
    pub prepared_input_digest: String,
    pub source_snapshot_sha256: String,
    pub input_base_fingerprint: String,
    pub snapshot_receipt_id: String,
    pub snapshot_receipt_expires_at: String,
}

/// Admit the unchanged governed handoff; local validity never grants publication authority.
pub fn validate_governed_intake(intake: &GovernedCityIntake) -> Result<()> {
    if intake.schema_version != GOVERNED_CITY_INTAKE_SCHEMA_VERSION {
        bail!("not a governed Solar intake v1");
    }
    intake
        .city_input
        .validate()
        .context("validating governed city input")?;
    let authority = &intake.authority;
    if !lower_sha256(&authority.source_snapshot_sha256)
        || !lower_sha256(&authority.input_base_fingerprint)
        || !canonical_uuid(&authority.snapshot_receipt_id)
        || authority.snapshot_bytes == 0
        || authority.snapshot_bytes > MAX_GOVERNED_SNAPSHOT_BYTES
        || authority.source_document_count == 0
        || authority.source_document_count > 512
        || authority.media_download_count > 512
    {
        bail!("invalid governed publication authority");
    }
    chrono::DateTime::parse_from_rfc3339(&authority.firestore_read_time)
        .context("invalid Firestore read time")?;
    let expires = chrono::DateTime::parse_from_rfc3339(&authority.snapshot_receipt_expires_at)
        .context("invalid snapshot receipt expiry")?;
    if expires <= chrono::Utc::now() {
        bail!("expired snapshot receipt; capture the governed city again");
    }
    if intake.city_input.revision.source != "ds-brain"
        || intake.city_input.revision.revision.as_deref()
            != Some(authority.input_base_fingerprint.as_str())
        || project_from_root(&intake.city_input.identity.root)?
            != intake.city_input.identity.project_id
    {
        bail!("governed intake disagrees with its publication authority");
    }
    anyhow::ensure!(
        ds_solar_contracts::is_solar_id(&intake.city_input.identity.city_id),
        "governed city id is not a safe single path segment"
    );
    Ok(())
}

/// Resolve only an exact verified local reference bundle. A miss refuses: no
/// provider, refresh, fixture or ambient executable can be attached here.
pub fn prepare_cached(
    intake: GovernedCityIntake,
    cache: &Path,
) -> Result<(PreparedSolarCityInput, PreparedPublicationClaim)> {
    validate_governed_intake(&intake)?;
    let cache = DirectoryReferenceCache::new(cache);
    let prepared = ReferencePreparer::new().with_cache(&cache).prepare(
        intake.city_input,
        chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string(),
    )?;
    let claim = prepared_publication_claim(&prepared.input, &intake.authority)?;
    Ok((prepared.input, claim))
}

pub fn prepared_publication_claim(
    prepared: &PreparedSolarCityInput,
    authority: &GovernedIntakeAuthority,
) -> Result<PreparedPublicationClaim> {
    if prepared.city.revision.source != "ds-brain"
        || prepared.city.revision.revision.as_deref()
            != Some(authority.input_base_fingerprint.as_str())
    {
        bail!("prepared Solar input disagrees with its governed intake claim");
    }
    Ok(PreparedPublicationClaim {
        schema_version: "ds-solar.prepared-publication-claim/v1".to_string(),
        claim_kind: "server-revalidated-publication-handoff".to_string(),
        project_id: prepared.city.identity.project_id.clone(),
        city_id: prepared.city.identity.city_id.clone(),
        city_content_digest: prepared.city.content_digest.clone(),
        prepared_input_digest: prepared.input_digest.clone(),
        source_snapshot_sha256: authority.source_snapshot_sha256.clone(),
        input_base_fingerprint: authority.input_base_fingerprint.clone(),
        snapshot_receipt_id: authority.snapshot_receipt_id.clone(),
        snapshot_receipt_expires_at: authority.snapshot_receipt_expires_at.clone(),
    })
}

/// Load prepared inputs from a directory, in city order.
pub fn load_prepared(directory: &Path, wanted: &[String]) -> Result<Vec<PreparedSolarCityInput>> {
    let mut requested = BTreeSet::new();
    for city_id in wanted {
        safe_city_id(city_id)?;
        if !requested.insert(city_id.as_str()) {
            bail!("--city repeats city id {city_id:?}");
        }
    }
    let mut paths: Vec<PathBuf> = std::fs::read_dir(directory)
        .with_context(|| format!("reading {}", directory.display()))?
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| path.to_string_lossy().ends_with(".prepared.json"))
        .collect();
    paths.sort();

    let mut prepared = Vec::new();
    let mut loaded = BTreeSet::new();
    for path in paths {
        let input: PreparedSolarCityInput = serde_json::from_slice(&crate::files::read_bounded(
            &path,
            MAX_PREPARED_INPUT_BYTES,
            "prepared Solar city input",
        )?)
        .with_context(|| format!("parsing {}", path.display()))?;
        safe_city_id(&input.city.identity.city_id)
            .with_context(|| format!("validating {}", path.display()))?;
        if !loaded.insert(input.city.identity.city_id.clone()) {
            bail!(
                "prepared directory repeats city id {:?}",
                input.city.identity.city_id
            );
        }
        if wanted.is_empty() || wanted.contains(&input.city.identity.city_id) {
            prepared.push(input);
        }
    }
    if prepared.is_empty() {
        bail!(
            "no prepared inputs in {} — run `ds-solar prepare` first",
            directory.display()
        );
    }
    let selected = prepared
        .iter()
        .map(|input| input.city.identity.city_id.as_str())
        .collect::<BTreeSet<_>>();
    let missing = requested.difference(&selected).copied().collect::<Vec<_>>();
    if !missing.is_empty() {
        bail!(
            "requested prepared cities are missing from {}: {}",
            directory.display(),
            missing.join(", ")
        );
    }
    prepared.sort_by(|left, right| left.city.identity.city_id.cmp(&right.city.identity.city_id));
    Ok(prepared)
}

/// Admit every selected input before a run creates progress, effects or output.
/// Local claims beside the inputs are deliberately not consumed or promoted.
pub fn load_prepared_for_run(
    directory: &Path,
    wanted: &[String],
) -> Result<Vec<PreparedSolarCityInput>> {
    let prepared = load_prepared(directory, wanted)?;
    for input in &prepared {
        if let Err(error) = ds_solar_contracts::require_prepared(input) {
            bail!("{}: {}", input.city.identity.city_id, error.message);
        }
    }
    Ok(prepared)
}

fn safe_city_id(city_id: &str) -> Result<()> {
    if !ds_solar_contracts::is_solar_id(city_id) {
        bail!(
            "city input id {:?} is not a safe single path segment",
            city_id
        );
    }
    Ok(())
}
