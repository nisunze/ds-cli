//! Explicit local analytical sources. No transport or provider fallback exists
//! here: the host supplies a pinned project asset and producer-completed coverage.
use crate::{Acquisition, Failure};
use ds_command_kernel::{
    project_dataset_cache::{
        self as policy, Coverage, Dataset, Quality, Scope, REFERENCE_CACHE_PROVIDER,
    },
    reference_cache::Identity,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs::File,
    io::{Read, Write},
    path::{Path, PathBuf},
};

const MAX_ARTIFACT_BYTES: u64 = 512 * 1024 * 1024;
const MAX_SOURCE_ROWS: usize = 500_000;
const BATCH_ROWS: usize = 4096;
const MAX_GEOMETRY_NODES: usize = 1_000_000;
const QUERY_ROWS: usize = 10_000;

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Origin {
    GoogleOpenBuildings,
    MicrosoftBuildings,
    #[serde(rename = "rwanda_dem_10m")]
    RwandaDem10m,
    AwsTerrarium,
    Custom,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Kind {
    Footprints,
    Contours {
        minor_interval_m: u16,
        index_interval_m: u16,
        sample_spacing_m: f64,
    },
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DigestAlgorithm {
    Sha256,
    Crc32c,
}

/// Producer input pinned separately from the generated contour/footprint file.
/// Rwanda's published COG has an exact CRC32C; no SHA-256 is invented for it.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ProducerSource {
    pub revision: String,
    pub digest_algorithm: DigestAlgorithm,
    pub digest: String,
}

/// A host-selected asset revision, never an inferred provider or coverage.
/// `artifact_path` is a local transport address and does not enter identity.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Source {
    pub schema: String,
    pub project_id: String,
    pub asset_id: String,
    pub revision: String,
    pub actual_source: Origin,
    pub producer_source: ProducerSource,
    pub content: Kind,
    pub artifact_path: PathBuf,
    pub artifact_sha256: String,
    pub completed_coverage: Coverage,
}

fn sha(value: &[u8]) -> String {
    format!("{:x}", Sha256::digest(value))
}
fn text(value: &str) -> bool {
    !value.is_empty() && value.len() <= 200 && !value.chars().any(char::is_control)
}
fn refusal(message: impl std::fmt::Display) -> Failure {
    Failure::AcquisitionFailed(message.to_string())
}
/// The Network engine decodes a held artifact. The converter's own refusal
/// keeps its words; an engine that is absent, too old or failed names its code
/// (`network_engine_missing`, `network_engine_update_required`, ...).
fn engine(failure: ds_network_client::NetworkFailure) -> Failure {
    if failure.is_columnar_refusal() {
        refusal(failure.message)
    } else {
        refusal(format!("{}: {}", failure.code(), failure.message))
    }
}

impl Source {
    pub fn validate(&self, scope: &Scope) -> Result<(), Failure> {
        if self.schema != "ds.local-analytical-source/v1"
            || self.project_id != scope.project
            || !text(&self.asset_id)
            || !text(&self.revision)
            || !text(&self.producer_source.revision)
            || self.producer_source.digest.len()
                != match self.producer_source.digest_algorithm {
                    DigestAlgorithm::Sha256 => 64,
                    DigestAlgorithm::Crc32c => 8,
                }
            || !self
                .producer_source
                .digest
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            || !self.artifact_path.is_absolute()
            || self.artifact_sha256.len() != 64
            || !self
                .artifact_sha256
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            || self.completed_coverage.is_empty()
        {
            return Err(refusal("local analytical source needs an exact project/asset/revision, SHA-256, absolute path and producer-completed coverage"));
        }
        self.completed_coverage.rects().map_err(refusal)?;
        match (&self.content, self.actual_source) {
            (Kind::Footprints, Origin::GoogleOpenBuildings | Origin::MicrosoftBuildings | Origin::Custom) => {},
            (Kind::Contours { minor_interval_m, index_interval_m, sample_spacing_m }, Origin::RwandaDem10m | Origin::AwsTerrarium | Origin::Custom)
                if *minor_interval_m > 0 && *index_interval_m >= *minor_interval_m
                    && *index_interval_m <= 1000 && *index_interval_m % *minor_interval_m == 0
                    && sample_spacing_m.is_finite() && (5.0..=250.0).contains(sample_spacing_m) => {},
            _ => return Err(refusal("local source origin does not match footprints/contours or their declared intervals")),
        }
        Ok(())
    }

    pub fn identity(&self) -> Identity {
        Identity {
            resource_id: sha(
                json!({"schema":self.schema,"project":self.project_id,"asset":self.asset_id,
                "actual_source":self.actual_source,"content":self.content})
                .to_string()
                .as_bytes(),
            ),
            version: sha(
                json!({"revision":self.revision,"artifact_sha256":self.artifact_sha256,"producer_source":self.producer_source,
                "completed_coverage":self.completed_coverage})
                .to_string()
                .as_bytes(),
            ),
        }
    }

    /// Exact resource/revision room, separated from every other source origin.
    pub fn dataset(&self) -> Dataset {
        let identity = self.identity();
        Dataset {
            id: identity.resource_id.clone(),
            provider: REFERENCE_CACHE_PROVIDER.into(),
            quality: Quality::Source,
            parameters: BTreeMap::from([
                ("resource_id".into(), json!(identity.resource_id)),
                ("local_source_version".into(), json!(identity.version)),
                ("asset_id".into(), json!(self.asset_id)),
                ("actual_source".into(), json!(self.actual_source)),
                ("producer_source".into(), json!(self.producer_source)),
                ("content".into(), json!(self.content)),
            ]),
            source_version: identity.version,
            crs: "EPSG:4326".into(),
        }
    }

    pub(crate) fn bind(&self, scope: &Scope, dataset: &Dataset) -> Result<(), Failure> {
        self.validate(scope)?;
        if self.dataset() != *dataset {
            return Err(refusal(
                "local analytical source differs from the selected dataset identity",
            ));
        }
        Ok(())
    }

    pub(crate) fn covers(&self, area: &Value) -> Result<(), Failure> {
        let bounds = ds_command_kernel::reference_cache::area_bounds(area).map_err(refusal)?;
        if !policy::assess(
            &self.completed_coverage,
            &Coverage {
                cells: vec![bounds],
            },
        )
        .map_err(refusal)?
        .covered
        {
            return Err(refusal("local analytical source has no completed coverage for this area; explicitly acquire or supply the missing asset coverage"));
        }
        Ok(())
    }

    /// Install the analytical input and its derived index only for a consumer
    /// that actually has missing coverage. A warm project seed never calls this.
    pub(crate) fn prepare(&self, root: &Path) -> Result<(), Failure> {
        let identity = self.identity();
        if crate::bundle::installed(root, &identity)? {
            return Ok(());
        }
        ds_layer_store::private::create_dir_all(root).map_err(crate::store)?;
        let staged = tempfile::tempdir_in(root).map_err(crate::store)?;
        let mut index = ds_layer_store::reference_cache::BundleIndexWriter::new(
            staged.path(),
            identity.clone(),
        )
        .map_err(crate::store)?;
        let retained = index.analytical_artifact_path().map_err(crate::store)?;
        let bytes = copy_verified(&self.artifact_path, &retained, &self.artifact_sha256)?;
        // The Network engine decodes the retained artifact; this owner admits
        // what it reports and indexes the features it returns.
        let held = std::fs::read(&retained).map_err(crate::store)?;
        let client =
            ds_network_client::CapturedNetworkClient::capture(1, &|| false).map_err(engine)?;
        let name = ds_network_contracts::columnar_io::COLUMNAR_ARTIFACT_INPUT;
        let summary = client
            .geoparquet_summary(name, &held, &|| false)
            .map_err(engine)?;
        let geo: Value = serde_json::from_str(summary.geo.as_deref().ok_or_else(|| {
            refusal("local analytical source must explicitly declare canonical CRS84")
        })?)
        .map_err(refusal)?;
        if geo["primary_column"] != ds_columnar::PARQUET_GEOMETRY_COLUMN
            || geo["columns"][ds_columnar::PARQUET_GEOMETRY_COLUMN]["encoding"] != "WKB"
            || geo["columns"][ds_columnar::PARQUET_GEOMETRY_COLUMN]["crs"]
                != ds_columnar::contract::crs84_projjson()
        {
            return Err(refusal(
                "local analytical source must carry the native canonical CRS84/WKB admission",
            ));
        }
        if summary.conversion.is_none()
            || summary.source_digest.is_none()
            || summary.feature_count > MAX_SOURCE_ROWS
        {
            return Err(refusal(
                "local source must be one native-converted analytical artifact with at most 500000 rows",
            ));
        }
        let mut ids = BTreeMap::new();
        let mut admitted = Ok(());
        let mut admit = |feature: Value| -> Result<(), Failure> {
            self.admit_feature(&feature)?;
            let id = feature["id"].as_str().ok_or_else(|| {
                refusal("local source feature needs its stable analytical identity")
            })?;
            let digest = sha(feature.to_string().as_bytes());
            if let Some(previous) = ids.insert(id.to_owned(), digest.clone()) {
                if previous != digest {
                    return Err(refusal(
                        "local source has conflicting rows for one stable feature identity",
                    ));
                }
                return Ok(());
            }
            index.push(&feature).map_err(refusal)
        };
        client
            .geoparquet_features(
                name,
                &held,
                BATCH_ROWS,
                MAX_GEOMETRY_NODES,
                &|| false,
                &mut |feature| match admit(feature) {
                    Ok(()) => std::ops::ControlFlow::Continue(()),
                    Err(failure) => {
                        admitted = Err(failure);
                        std::ops::ControlFlow::Break(())
                    }
                },
            )
            .map_err(engine)?;
        admitted?;
        index.finish_analytical_artifact(bytes).map_err(refusal)?;
        ds_layer_store::reference_cache::publish_staged(root, staged.path(), &identity)
            .map_err(crate::store)?;
        Ok(())
    }

    fn admit_feature(&self, feature: &Value) -> Result<(), Failure> {
        let geometry = feature["geometry"]["type"].as_str();
        match &self.content {
            Kind::Footprints if matches!(geometry, Some("Polygon" | "MultiPolygon")) => Ok(()),
            Kind::Contours {
                minor_interval_m,
                index_interval_m,
                ..
            } if matches!(geometry, Some("LineString" | "MultiLineString")) => {
                let elevation = feature["properties"]["elevation"]
                    .as_f64()
                    .ok_or_else(|| refusal("local contour needs its finite elevation"))?;
                let kind = feature["properties"]["contour_kind"].as_str();
                let multiple = |step: u16| {
                    (elevation / f64::from(step) - (elevation / f64::from(step)).round()).abs()
                        < 1e-8
                };
                if !elevation.is_finite()
                    || !multiple(*minor_interval_m)
                    || !matches!(kind, Some("index" | "intermediate"))
                    || (kind == Some("index")) != multiple(*index_interval_m)
                {
                    return Err(refusal(
                        "local contour class/elevation disagrees with the declared intervals",
                    ));
                }
                Ok(())
            }
            _ => Err(refusal(
                "local analytical geometry does not match its declared footprints/contours kind",
            )),
        }
    }

    pub(crate) fn acquire(
        &self,
        root: &Path,
        area: &Value,
    ) -> Result<Option<Acquisition>, Failure> {
        self.covers(area)?;
        #[cfg(test)]
        counts::query();
        let identity = self.identity();
        let result = ds_layer_store::reference_cache::execute_at(
            root,
            json!({"action":"query","identity":identity,"area":area,"limit":QUERY_ROWS}),
        )
        .map_err(crate::store)?;
        if result["truncated"] == true {
            return Ok(None);
        }
        if result["cache"] != "local" || result["version"] != identity.version {
            return Err(refusal(
                "local analytical index returned a different source revision",
            ));
        }
        let features = result["features"]
            .as_array()
            .cloned()
            .ok_or_else(|| refusal("local analytical index returned no feature array"))?;
        Ok(Some(Acquisition {
            source_version: identity.version.clone(),
            source_receipt: json!({"kind":"local_analytical_asset_subset","asset_id":self.asset_id,
                "revision":self.revision,"actual_source":self.actual_source,"content":self.content,"producer_source":self.producer_source,
                "artifact_sha256":self.artifact_sha256,"resource_id":identity.resource_id,
                "resource_version":identity.version,"completed_source_coverage":self.completed_coverage,
                "area":area,"feature_count":features.len()}),
            features,
        }))
    }
}

fn copy_verified(source: &Path, destination: &Path, expected: &str) -> Result<u64, Failure> {
    let metadata = std::fs::symlink_metadata(source).map_err(|error| Failure::NotHeld(format!(
        "local analytical input is unavailable; explicitly acquire the selected asset revision: {error}")))?;
    if !metadata.is_file()
        || metadata.file_type().is_symlink()
        || metadata.len() == 0
        || metadata.len() > MAX_ARTIFACT_BYTES
    {
        return Err(refusal(
            "local analytical input must be a regular file of at most 512 MiB",
        ));
    }
    #[cfg(test)]
    counts::source();
    let mut input = File::open(source).map_err(refusal)?;
    let mut output = ds_layer_store::private::open_options()
        .write(true)
        .create_new(true)
        .open(destination)
        .map_err(crate::store)?;
    let mut hasher = Sha256::new();
    let mut bytes = 0;
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let read = input.read(&mut buffer).map_err(refusal)?;
        if read == 0 {
            break;
        }
        bytes += read as u64;
        if bytes > MAX_ARTIFACT_BYTES {
            return Err(refusal("local analytical input grew beyond its byte bound"));
        }
        hasher.update(&buffer[..read]);
        output.write_all(&buffer[..read]).map_err(crate::store)?;
    }
    if format!("{:x}", hasher.finalize()) != expected {
        return Err(refusal(
            "local analytical input differs from its exact SHA-256 pin",
        ));
    }
    output.sync_all().map_err(crate::store)?;
    Ok(bytes)
}

#[cfg(test)]
mod counts {
    use std::cell::Cell;
    thread_local! { static COUNTS: Cell<(usize,usize)> = const { Cell::new((0,0)) }; }
    pub fn source() {
        COUNTS.with(|c| {
            let (s, q) = c.get();
            c.set((s + 1, q));
        });
    }
    pub fn query() {
        COUNTS.with(|c| {
            let (s, q) = c.get();
            c.set((s, q + 1));
        });
    }
    pub fn take() -> (usize, usize) {
        COUNTS.with(|c| c.replace((0, 0)))
    }
}

#[cfg(test)]
#[path = "local_source_tests.rs"]
mod tests;
