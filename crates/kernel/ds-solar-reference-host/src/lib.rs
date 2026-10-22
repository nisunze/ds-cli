//! Bounded native acquisition of digest-pinned Solar reference bundles.
//!
//! Preparation calls this application effect owner; offline Solar computation
//! and its IO contracts have no HTTP client or provider feature.

use ds_solar_contracts::{BundleBytes, SolarError, SolarResult};
pub mod preparation;
pub mod provider;
pub mod reference_import;
pub use preparation::{BundleResolution, Preparation, ReferencePreparer, Refresh};
pub use provider::{FixtureReferenceProvider, ReferenceBundleProvider, ReferenceRequest};
use serde::Deserialize;

/// The published inventory `ds-solar-weather` answers with.
///
/// It is not the bytes. The four artifacts live in the project's default
/// Firebase bucket under a content-addressed prefix, and this document is
/// the digest-bound list of where they are and what they must hash to.
#[derive(Debug, Clone, Deserialize)]
struct PublishedBundle {
    schema_version: String,
    source_key: String,
    bundle_digest: String,
    artifacts: Vec<PublishedArtifact>,
}

#[derive(Debug, Clone, Deserialize)]
struct PublishedArtifact {
    name: String,
    bytes: usize,
    sha256: String,
    url: String,
}

/// A provider backed by the `ds-solar-weather` Cloud Run service.
///
/// Authentication is the adapter's job, not the Solar domain's: the caller
/// hands over an already-minted bearer token and this type does nothing but
/// attach it. It never mints, refreshes, reads or stores a credential.
#[derive(Debug, Clone)]
pub struct HttpReferenceProvider {
    base_url: String,
    bearer_token: Option<String>,
    api_key: Option<String>,
    client: reqwest::blocking::Client,
}

impl HttpReferenceProvider {
    /// Point at a `ds-solar-weather` deployment.
    pub fn new(base_url: impl Into<String>, bearer_token: Option<String>) -> SolarResult<Self> {
        let client = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(180))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|err| {
                SolarError::weather_unavailable(format!("reference HTTP client: {err}"))
            })?;
        Ok(Self {
            base_url: base_url.into().trim_end_matches('/').to_string(),
            bearer_token,
            api_key: None,
            client,
        })
    }

    /// Attach the public API Gateway key used by the authenticated
    /// ds-brain relay. The direct service does not require this header, so
    /// CLI callers can keep using [`Self::new`].
    #[must_use]
    pub fn with_api_key(mut self, api_key: Option<String>) -> Self {
        self.api_key = api_key.filter(|value| !value.trim().is_empty());
        self
    }

    fn download(&self, artifact: &PublishedArtifact) -> SolarResult<Vec<u8>> {
        // The URL is already signed and already time-bounded. The bearer
        // token is deliberately *not* attached: a signed object URL carries
        // its own authority and forwarding a Firebase identity to Google
        // Cloud Storage would widen what the token is presented to.
        let response = self.client.get(&artifact.url).send().map_err(|err| {
            SolarError::weather_unavailable(format!(
                "{} could not be downloaded: {}",
                artifact.name,
                err.without_url()
            ))
        })?;
        if !response.status().is_success() {
            return Err(SolarError::weather_unavailable(format!(
                "{} answered {}",
                artifact.name,
                response.status()
            )));
        }
        if artifact.bytes as u64 > ds_solar_io::MAX_REFERENCE_ARTIFACT_BYTES {
            return Err(SolarError::reference_unit_invalid(
                "Reference artifact exceeds 16 MiB",
            ));
        }
        let body = bounded_response(response, artifact.bytes as u64, &artifact.name)?;
        if body.len() != artifact.bytes {
            return Err(SolarError::reference_unit_invalid(format!(
                "{} arrived as {} bytes, the inventory declares {}",
                artifact.name,
                body.len(),
                artifact.bytes
            )));
        }
        let actual = ds_solar_contracts::digest::hex_sha256(&body);
        if actual != artifact.sha256 {
            return Err(SolarError::reference_unit_invalid(format!(
                "{} hashes to {actual}, the inventory declares {}",
                artifact.name, artifact.sha256
            )));
        }
        Ok(body)
    }
}

fn bounded_response(
    response: reqwest::blocking::Response,
    max: u64,
    label: &str,
) -> SolarResult<Vec<u8>> {
    use std::io::Read;
    if response.content_length().is_some_and(|n| n > max) {
        return Err(SolarError::reference_unit_invalid(format!(
            "{label} exceeds its declared response bound"
        )));
    }
    let mut body = Vec::new();
    response
        .take(max + 1)
        .read_to_end(&mut body)
        .map_err(|_| SolarError::weather_unavailable(format!("{label} response was unreadable")))?;
    if body.len() as u64 > max {
        return Err(SolarError::reference_unit_invalid(format!(
            "{label} exceeds its declared response bound"
        )));
    }
    Ok(body)
}

impl ReferenceBundleProvider for HttpReferenceProvider {
    fn fetch(&self, request: &ReferenceRequest) -> SolarResult<BundleBytes> {
        let url = format!("{}/api/v1/solar/reference-unit", self.base_url);
        let mut builder = self.client.post(&url).json(request);
        if let Some(token) = &self.bearer_token {
            builder = builder.bearer_auth(token);
        }
        if let Some(api_key) = &self.api_key {
            builder = builder.header("x-api-key", api_key);
        }
        let response = builder.send().map_err(|err| {
            SolarError::weather_unavailable(format!("{url} could not be reached: {err}"))
        })?;
        let status = response.status();
        let bytes = bounded_response(response, 256 * 1024, "reference inventory")?;
        let body = std::str::from_utf8(&bytes)
            .map_err(|_| SolarError::reference_unit_invalid("Reference inventory is not UTF-8"))?;
        if !status.is_success() {
            return Err(SolarError::weather_unavailable(format!(
                "{url} answered {status}: {}",
                body.chars().take(400).collect::<String>()
            )));
        }
        let published: PublishedBundle = serde_json::from_str(body).map_err(|err| {
            SolarError::reference_unit_invalid(format!("{url} returned a non-contract body: {err}"))
        })?;
        if published.schema_version != ds_solar_contracts::REFERENCE_BUNDLE_SCHEMA_VERSION {
            return Err(SolarError::schema_incompatible(format!(
                "{url} publishes '{}' but this build speaks '{}'",
                published.schema_version,
                ds_solar_contracts::REFERENCE_BUNDLE_SCHEMA_VERSION
            )));
        }

        if published.artifacts.len() != ds_solar_io::cache::ARTIFACT_NAMES.len()
            || ds_solar_io::cache::ARTIFACT_NAMES.iter().any(|name| {
                published
                    .artifacts
                    .iter()
                    .filter(|a| a.name == *name)
                    .count()
                    != 1
            })
            || published.artifacts.iter().any(|a| {
                a.bytes == 0
                    || a.bytes as u64 > ds_solar_io::MAX_REFERENCE_ARTIFACT_BYTES
                    || a.sha256.len() != 64
                    || !a.sha256.bytes().all(|b| b.is_ascii_hexdigit())
            })
        {
            return Err(SolarError::reference_unit_invalid(
                "Reference inventory must contain exactly four bounded digest-pinned artifacts",
            ));
        }
        let mut artifacts = BundleBytes::default();
        for name in ds_solar_io::cache::ARTIFACT_NAMES {
            let listed = published
                .artifacts
                .iter()
                .find(|artifact| artifact.name == name)
                .ok_or_else(|| {
                    SolarError::reference_unit_invalid(format!(
                        "{url} published no '{name}' for {}",
                        published.source_key
                    ))
                })?;
            let body = self.download(listed)?;
            match name {
                "weather.json" => artifacts.weather_json = body,
                "reference-unit.json" => artifacts.reference_unit_json = body,
                "reference-unit.parquet" => artifacts.reference_unit_parquet = body,
                "manifest.json" => artifacts.manifest_json = body,
                _ => unreachable!("ARTIFACT_NAMES is exhaustive"),
            }
        }
        // The manifest that arrived must agree with the inventory that
        // announced it, or the two halves of the answer disagree.
        let opened = ds_solar_io::reference::open(&artifacts)?;
        if opened.reference_unit.manifest.source_key != published.source_key
            || opened.reference_unit.manifest.bundle_digest != published.bundle_digest
        {
            return Err(SolarError::reference_unit_invalid(format!(
                "{url} announced bundle {} but published {}",
                published.bundle_digest, opened.reference_unit.manifest.bundle_digest
            )));
        }
        Ok(artifacts)
    }
}
