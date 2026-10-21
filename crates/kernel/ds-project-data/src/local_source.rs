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
        let mut reader =
            ds_columnar::ArtifactReader::open(&retained, BATCH_ROWS).map_err(refusal)?;
        let schema = reader.schema();
        let geo: Value = serde_json::from_str(schema.metadata().get("geo").ok_or_else(|| {
            refusal("local analytical source must explicitly declare canonical CRS84")
        })?)
        .map_err(refusal)?;
        if geo["primary_column"] != ds_columnar::PARQUET_GEOMETRY_COLUMN
            || geo["columns"][ds_columnar::PARQUET_GEOMETRY_COLUMN]["encoding"] != "WKB"
            || geo["columns"][ds_columnar::PARQUET_GEOMETRY_COLUMN]["crs"]
                != ds_columnar::geoparquet::crs84_projjson()
        {
            return Err(refusal(
                "local analytical source must carry the native canonical CRS84/WKB admission",
            ));
        }
        if reader.summary().conversion.is_none()
            || reader.summary().source_digest.is_none()
            || reader.summary().feature_count > MAX_SOURCE_ROWS
        {
            return Err(refusal(
                "local source must be one native-converted analytical artifact with at most 500000 rows",
            ));
        }
        let mut ids = BTreeMap::new();
        while let Some(batch) = reader.next_batch().map_err(refusal)? {
            for feature in
                ds_columnar::batch_to_features(&batch, MAX_GEOMETRY_NODES).map_err(refusal)?
            {
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
                    continue;
                }
                index.push(&feature).map_err(refusal)?;
            }
        }
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
mod tests {
    use super::*;
    use crate::{BundleReceipt, DeclaredSource, Hosts, Mode, Provider};
    use policy::{buffer_policy, Extent};

    struct NoCloud(usize);
    impl Provider for NoCloud {
        fn acquire(&mut self, _: &Dataset, _: &Value) -> Result<Acquisition, Failure> {
            self.0 += 1;
            Err(refusal("no cloud provider is admitted"))
        }
    }
    fn scope() -> Scope {
        Scope {
            principal: "fixture-user".into(),
            project: "fixture-project".into(),
        }
    }
    fn footprint(id: &str, x: f64) -> Value {
        json!({"type":"Feature","id":id,"properties":{"area_in_meters":40.,"confidence":0.9},
            "geometry":{"type":"Polygon","coordinates":[[[x,-2.],[x+0.0001,-2.],[x+0.0001,-1.9999],[x,-1.9999],[x,-2.]]]}})
    }
    fn source(dir: &Path, features: Vec<Value>) -> Source {
        let bytes =
            serde_json::to_vec(&json!({"type":"FeatureCollection","features":features})).unwrap();
        let converted =
            ds_columnar::source_to_geoparquet("fixture.geojson", &bytes, None, None).unwrap();
        let path = dir.join("source-input.parquet");
        std::fs::write(&path, converted.parquet).unwrap();
        Source {
            schema: "ds.local-analytical-source/v1".into(),
            project_id: scope().project,
            asset_id: "fixture-footprints".into(),
            revision: "revision-1".into(),
            actual_source: Origin::GoogleOpenBuildings,
            producer_source: ProducerSource {
                revision: "fixture-publisher-revision-1".into(),
                digest_algorithm: DigestAlgorithm::Sha256,
                digest: sha(&bytes),
            },
            content: Kind::Footprints,
            artifact_sha256: ds_columnar::digest_file(&path).unwrap(),
            artifact_path: path,
            completed_coverage: Coverage {
                cells: vec![[29.9, -2.1, 30.2, -1.9]],
            },
        }
    }
    fn extents(second: bool) -> Vec<Extent> {
        let mut result = vec![Extent {
            name: "transformer-a".into(),
            bounds: Some([30., -2., 30.001, -1.999]),
        }];
        if second {
            result.push(Extent {
                name: "transformer-b".into(),
                bounds: Some([30.06, -2., 30.061, -1.999]),
            });
        }
        result
    }
    fn run(
        root: &Path,
        source: &Source,
        extents: &[Extent],
        read: bool,
        refresh: bool,
    ) -> Result<crate::EnsureOutcome, Failure> {
        let mut provider = NoCloud(0);
        let mut downloads = 0;
        let mut fetch = |_: &BundleReceipt, _: &Path| {
            downloads += 1;
            Err("no download is admitted".into())
        };
        let mut hosts = Hosts {
            provider: &mut provider,
            fetch: &mut fetch,
        };
        let dataset = source.dataset();
        let selected = DeclaredSource::LocalAnalytical(Box::new(source.clone()));
        let policy = buffer_policy(&[200.]).unwrap();
        let result = if refresh {
            crate::refresh(
                root,
                &scope(),
                &dataset,
                &selected,
                &policy,
                extents,
                &[],
                &mut hosts,
            )
        } else {
            crate::ensure(
                root,
                &scope(),
                &dataset,
                &selected,
                &policy,
                extents,
                &[],
                if read {
                    Mode::Read
                } else {
                    Mode::Acquire(&mut hosts)
                },
            )
        };
        assert_eq!(provider.0, 0, "local inputs never call the cloud provider");
        assert_eq!(
            downloads, 0,
            "local inputs never download a reference bundle"
        );
        result
    }
    fn status(root: &Path, source: &Source) -> Value {
        ds_layer_store::project_dataset_cache::execute_at(
            root,
            json!({"action":"status",
            "dataset_key":policy::dataset_key(&scope(), &source.dataset()),
            "principal":scope().principal,"project":scope().project}),
        )
        .unwrap()
    }

    #[test]
    fn local_arrow_source_seeds_once_patches_only_gaps_and_reads_offline() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("held");
        let first_feature = footprint("footprint-a", 30.0005);
        let input = source(
            dir.path(),
            vec![
                first_feature.clone(),
                first_feature,
                footprint("footprint-b", 30.0605),
            ],
        );
        counts::take();
        let cold = run(&root, &input, &extents(false), false, false).unwrap();
        let (source_reads, index_reads) = counts::take();
        assert!(cold.covered && cold.acquired > 0);
        assert_eq!(source_reads, 1);
        assert_eq!(index_reads, cold.acquired);
        assert_eq!(
            cold.features, 1,
            "duplicate stable source rows are not duplicate buildings"
        );
        std::fs::remove_file(&input.artifact_path).unwrap();
        let warm = run(&root, &input, &extents(false), false, false).unwrap();
        assert_eq!((warm.acquired, counts::take()), (0, (0, 0)));
        let offline = run(&root, &input, &extents(false), true, false).unwrap();
        assert!(offline.covered);
        assert_eq!((offline.acquired, counts::take()), (0, (0, 0)));
        let expanded = run(&root, &input, &extents(true), false, false).unwrap();
        let (source_reads, index_reads) = counts::take();
        assert!(expanded.covered && expanded.acquired > 0);
        assert_eq!(
            source_reads, 0,
            "the retained analytical asset/index survives original input removal"
        );
        assert_eq!(index_reads, expanded.acquired);
        assert_eq!(expanded.features, 2);
        let page = ds_layer_store::reference_cache::execute_at(&root,
            json!({"action":"query","identity":input.identity(),"area":input.completed_coverage.geometry(),"limit":10})).unwrap();
        assert_eq!(page["features"].as_array().unwrap().len(), 2);
        assert_eq!(page["features"][0]["id"], "footprint-a");
        let refreshed = run(&root, &input, &extents(true), false, true).unwrap();
        let (source_reads, index_reads) = counts::take();
        assert_eq!(source_reads, 0);
        assert_eq!(index_reads, refreshed.acquired);
        assert_eq!(refreshed.features, 2);
    }

    #[test]
    fn missing_input_coverage_and_changed_revisions_never_claim_completion() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("held");
        let input = source(dir.path(), vec![footprint("a", 30.0005)]);
        let cold = run(&root, &input, &extents(false), false, false).unwrap();
        let before = status(&root, &input)["completed"].clone();
        let mut out_of_coverage = extents(false);
        out_of_coverage.push(Extent {
            name: "outside-input".into(),
            bounds: Some([31., -2., 31.001, -1.999]),
        });
        counts::take();
        assert!(run(&root, &input, &out_of_coverage, false, false)
            .unwrap_err()
            .message()
            .contains("no completed coverage"));
        assert_eq!(counts::take(), (0, 0));
        assert_eq!(status(&root, &input)["completed"], before);
        let empty_expansion = run(&root, &input, &extents(true), false, false).unwrap();
        assert!(empty_expansion.covered && empty_expansion.acquired > 0);
        assert_eq!(empty_expansion.features, 1);
        assert_eq!(counts::take(), (0, empty_expansion.acquired));
        let mut changed = input.clone();
        changed.revision = "revision-2".into();
        assert_ne!(
            policy::dataset_key(&scope(), &input.dataset()),
            policy::dataset_key(&scope(), &changed.dataset())
        );
        let unseeded = run(&root, &changed, &extents(false), true, false).unwrap();
        assert!(!unseeded.covered && unseeded.acquired == 0);
        assert_eq!(counts::take(), (0, 0));
        assert!(cold.covered);
    }

    #[test]
    fn digest_failure_and_conflicting_source_ids_leave_no_completed_or_published_input() {
        for conflict in [false, true] {
            let dir = tempfile::tempdir().unwrap();
            let root = dir.path().join("held");
            let mut different = footprint("same", 30.0005);
            different["properties"]["area_in_meters"] = json!(99.);
            let mut input = source(
                dir.path(),
                if conflict {
                    vec![footprint("same", 30.0005), different]
                } else {
                    vec![footprint("a", 30.0005)]
                },
            );
            if !conflict {
                input.artifact_sha256 = "a".repeat(64);
            }
            assert!(run(&root, &input, &extents(false), false, false).is_err());
            assert_eq!(status(&root, &input)["completed"]["cells"], json!([]));
            assert!(!crate::bundle::installed(&root, &input.identity()).unwrap());
        }
    }

    #[test]
    fn explicitly_selected_contours_keep_their_surface_and_intervals() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("held");
        let line = |id, elevation, kind| {
            json!({"type":"Feature","id":id,
            "properties":{"elevation":elevation,"contour_kind":kind},
            "geometry":{"type":"LineString","coordinates":[[30.,-2.],[30.001,-1.999]]}})
        };
        let mut input = source(
            dir.path(),
            vec![
                line("minor", 1510., "intermediate"),
                line("index", 1500., "index"),
            ],
        );
        input.content = Kind::Contours {
            minor_interval_m: 10,
            index_interval_m: 50,
            sample_spacing_m: 10.,
        };
        input.actual_source = Origin::RwandaDem10m;
        input.producer_source = ProducerSource {
            revision: "fixture-dem-revision-1".into(),
            digest_algorithm: DigestAlgorithm::Crc32c,
            digest: "31dad180".into(),
        };
        let result = run(&root, &input, &extents(false), false, false).unwrap();
        assert_eq!(result.features, 2);
        assert_eq!(
            input.dataset().parameters["actual_source"],
            "rwanda_dem_10m"
        );
        assert_eq!(
            input.dataset().parameters["producer_source"]["digest"],
            "31dad180"
        );
        let mut changed_surface = input.clone();
        changed_surface.producer_source.digest = "31dad181".into();
        assert_ne!(input.identity().version, changed_surface.identity().version);
        changed_surface = input.clone();
        changed_surface.producer_source.revision = "fixture-dem-revision-2".into();
        assert_ne!(input.identity().version, changed_surface.identity().version);
        let mut alternate = input.clone();
        alternate.actual_source = Origin::AwsTerrarium;
        assert_ne!(
            input.identity().resource_id,
            alternate.identity().resource_id
        );
        counts::take();
        input.actual_source = Origin::GoogleOpenBuildings;
        assert!(run(&root, &input, &extents(false), false, false).is_err());
        assert_eq!(counts::take(), (0, 0));
    }

    #[test]
    fn cross_project_sources_refuse_before_any_source_access() {
        let dir = tempfile::tempdir().unwrap();
        let mut input = source(dir.path(), vec![footprint("a", 30.0005)]);
        input.project_id = "other-project".into();
        counts::take();
        assert!(run(
            &dir.path().join("held"),
            &input,
            &extents(false),
            false,
            false
        )
        .is_err());
        assert_eq!(counts::take(), (0, 0));
    }

    #[test]
    fn completed_empty_footprints_and_flat_contours_seed_once_without_fallback() {
        for content in [
            Kind::Footprints,
            Kind::Contours {
                minor_interval_m: 10,
                index_interval_m: 50,
                sample_spacing_m: 10.,
            },
        ] {
            let dir = tempfile::tempdir().unwrap();
            let root = dir.path().join("held");
            let mut input = source(dir.path(), vec![]);
            input.actual_source = if matches!(content, Kind::Footprints) {
                Origin::MicrosoftBuildings
            } else {
                Origin::RwandaDem10m
            };
            input.content = content;
            counts::take();
            let first = run(&root, &input, &extents(false), false, false).unwrap();
            assert!(first.covered && first.acquired > 0);
            assert_eq!(first.features, 0);
            assert_eq!(counts::take(), (1, first.acquired));
            assert!(crate::bundle::installed(&root, &input.identity()).unwrap());
            std::fs::remove_file(&input.artifact_path).unwrap();
            let warm = run(&root, &input, &extents(false), false, false).unwrap();
            assert!(warm.covered && warm.features == 0);
            assert_eq!((warm.acquired, counts::take()), (0, (0, 0)));
            let offline = run(&root, &input, &extents(false), true, false).unwrap();
            assert!(offline.covered && offline.features == 0);
            assert_eq!((offline.acquired, counts::take()), (0, (0, 0)));
        }
    }

    #[test]
    fn valid_artifact_digest_does_not_admit_an_altered_coordinate_reference_system() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("held");
        let mut input = source(dir.path(), vec![footprint("a", 30.0005)]);
        let mut bytes = std::fs::read(&input.artifact_path).unwrap();
        let mut altered = 0;
        for start in 0..bytes.len() - 5 {
            if &bytes[start..start + 5] == b"CRS84" {
                bytes[start..start + 5].copy_from_slice(b"CRS83");
                altered += 1;
            }
        }
        assert!(altered > 0);
        std::fs::write(&input.artifact_path, bytes).unwrap();
        input.artifact_sha256 = ds_columnar::digest_file(&input.artifact_path).unwrap();
        assert!(run(&root, &input, &extents(false), false, false)
            .unwrap_err()
            .message()
            .contains("CRS84"));
        assert_eq!(status(&root, &input)["completed"]["cells"], json!([]));
        assert!(!crate::bundle::installed(&root, &input.identity()).unwrap());
    }
}
