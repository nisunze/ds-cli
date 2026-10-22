use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use ds_elevation_host::FetchEvidence;
use ds_geo::projection::GridModelCrs;
use ds_terrain_sampling::{DerivedObservation, ProfileSample, SampledProfile, SamplingResult};
use serde::Serialize;
use serde::ser::{SerializeSeq, Serializer};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::{
    ArtifactEvidence, DERIVED_ROW_CLASS, InputEvidence, MAX_OUTPUT_BYTES, MAX_RECEIPT_BYTES,
    RouteResult, SamplingFileError, SamplingFileReceipt, SamplingFileRequest,
};

const OUTPUT_LIMIT_MESSAGE: &str = "terrain output exceeds admitted byte budget";

pub(crate) fn check_receipt(receipt: &SamplingFileReceipt) -> Result<(), SamplingFileError> {
    let bytes =
        serde_json::to_vec(receipt).map_err(|error| SamplingFileError::Io(error.to_string()))?;
    // Reserve room for the bounded set of artifact digests before acquisition.
    let limit = if receipt.files.is_empty() {
        MAX_RECEIPT_BYTES - 1_024
    } else {
        MAX_RECEIPT_BYTES
    };
    if bytes.len() > limit {
        return Err(SamplingFileError::InputTooLarge(
            "receipt metadata exceeds bounded inline size".into(),
        ));
    }
    Ok(())
}

pub(crate) fn profile_counts(result: &SamplingResult) -> (usize, usize) {
    let profiles = std::iter::once(&result.centerline).chain(&result.side_profiles);
    profiles
        .flat_map(|profile| &profile.samples)
        .fold((0, 0), |(supported, missing), sample| {
            if sample.elevation_m.is_some() {
                (supported + 1, missing)
            } else {
                (supported, missing + 1)
            }
        })
}

#[derive(Serialize)]
struct ClassifiedRow<'a, T> {
    row_class: &'static str,
    #[serde(flatten)]
    row: &'a T,
}

struct DerivedRows<'a, T>(&'a [T]);

impl<T: Serialize> Serialize for DerivedRows<'_, T> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut sequence = serializer.serialize_seq(Some(self.0.len()))?;
        for row in self.0 {
            sequence.serialize_element(&ClassifiedRow {
                row_class: DERIVED_ROW_CLASS,
                row,
            })?;
        }
        sequence.end()
    }
}

#[derive(Serialize)]
struct ProfileView<'a> {
    nominal_offset_m: f64,
    samples: DerivedRows<'a, ProfileSample>,
    supported_runs: &'a [ds_terrain_sampling::SupportedRun],
    gaps: &'a [ds_terrain_sampling::SurfaceGap],
    baseline_evidence: &'a ds_terrain_sampling::BaselineEvidence,
    maximum_sample_gap_m: f64,
}

impl<'a> From<&'a SampledProfile> for ProfileView<'a> {
    fn from(profile: &'a SampledProfile) -> Self {
        Self {
            nominal_offset_m: profile.nominal_offset_m,
            samples: DerivedRows(&profile.samples),
            supported_runs: &profile.supported_runs,
            gaps: &profile.gaps,
            baseline_evidence: &profile.baseline_evidence,
            maximum_sample_gap_m: profile.maximum_sample_gap_m,
        }
    }
}

struct SideViews<'a>(&'a [SampledProfile]);
impl Serialize for SideViews<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut sequence = serializer.serialize_seq(Some(self.0.len()))?;
        for profile in self.0 {
            sequence.serialize_element(&ProfileView::from(profile))?;
        }
        sequence.end()
    }
}

#[derive(Serialize)]
struct ResultView<'a> {
    seed: u64,
    route_length_m: f64,
    normalized_route_vertex_count: usize,
    query_count: usize,
    centerline: ProfileView<'a>,
    side_profiles: SideViews<'a>,
    scattered_observations: DerivedRows<'a, DerivedObservation>,
    side_observations: DerivedRows<'a, DerivedObservation>,
    engineering_density: &'a ds_terrain_sampling::EngineeringDensityEvidence,
    source_observations_mutated: bool,
}

impl<'a> From<&'a SamplingResult> for ResultView<'a> {
    fn from(result: &'a SamplingResult) -> Self {
        Self {
            seed: result.seed,
            route_length_m: result.route_length_m,
            normalized_route_vertex_count: result.normalized_route_vertex_count,
            query_count: result.query_count,
            centerline: ProfileView::from(&result.centerline),
            side_profiles: SideViews(&result.side_profiles),
            scattered_observations: DerivedRows(&result.scattered_observations),
            side_observations: DerivedRows(&result.side_observations),
            engineering_density: &result.engineering_density,
            source_observations_mutated: result.source_observations_mutated,
        }
    }
}

#[derive(Serialize)]
struct RouteView<'a> {
    route_id: &'a str,
    feature_id: &'a Option<Value>,
    feature_index: usize,
    part_index: usize,
    result: ResultView<'a>,
}

struct RouteViews<'a>(&'a [RouteResult]);
impl Serialize for RouteViews<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut sequence = serializer.serialize_seq(Some(self.0.len()))?;
        for route in self.0 {
            sequence.serialize_element(&RouteView {
                route_id: &route.route.route_id,
                feature_id: &route.route.feature_id,
                feature_index: route.route.feature_index,
                part_index: route.route.part_index,
                result: ResultView::from(&route.result),
            })?;
        }
        sequence.end()
    }
}

#[derive(Serialize)]
struct ResultsDocument<'a> {
    schema: &'static str,
    projected_crs: String,
    source: &'a Value,
    derived_row_class: &'static str,
    routes: RouteViews<'a>,
}

struct ArtifactWriter {
    file: File,
    hash: Sha256,
    bytes: u64,
    limit: u64,
}

impl Write for ArtifactWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() as u64 > self.limit.saturating_sub(self.bytes) {
            return Err(io::Error::other(OUTPUT_LIMIT_MESSAGE));
        }
        let written = self.file.write(bytes)?;
        self.hash.update(&bytes[..written]);
        self.bytes += written as u64;
        Ok(written)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.file.flush()
    }
}

fn write_error(error: impl std::fmt::Display) -> SamplingFileError {
    let detail = error.to_string();
    if detail.contains(OUTPUT_LIMIT_MESSAGE) {
        SamplingFileError::InputTooLarge(detail)
    } else {
        SamplingFileError::Io(detail)
    }
}

/// Removes only files this invocation created. An unexpected external file in
/// the directory prevents removal of that directory; it is never swept away.
struct CreatedOutput {
    directory: PathBuf,
    created_files: Vec<PathBuf>,
    committed: bool,
    bytes: u64,
    artifacts: Vec<ArtifactEvidence>,
}

impl CreatedOutput {
    fn new(directory: &Path) -> Result<Self, SamplingFileError> {
        crate::input::fresh_output(directory)?;
        fs::create_dir(directory).map_err(|error| {
            if error.kind() == io::ErrorKind::AlreadyExists {
                SamplingFileError::OutputExists(directory.to_string_lossy().into_owned())
            } else {
                SamplingFileError::Io(error.to_string())
            }
        })?;
        Ok(Self {
            directory: directory.to_path_buf(),
            created_files: Vec::new(),
            committed: false,
            bytes: 0,
            artifacts: Vec::new(),
        })
    }

    fn artifact<F>(&mut self, name: &str, write: F) -> Result<(), SamplingFileError>
    where
        F: FnOnce(&mut ArtifactWriter) -> Result<(), SamplingFileError>,
    {
        let path = self.directory.join(name);
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(|error| SamplingFileError::Io(error.to_string()))?;
        self.created_files.push(path);
        let mut writer = ArtifactWriter {
            file,
            hash: Sha256::new(),
            bytes: 0,
            limit: MAX_OUTPUT_BYTES.saturating_sub(self.bytes),
        };
        write(&mut writer)?;
        writer.flush().map_err(write_error)?;
        writer.file.sync_all().map_err(write_error)?;
        self.bytes += writer.bytes;
        self.artifacts.push(ArtifactEvidence {
            file: name.into(),
            bytes: writer.bytes,
            sha256: format!("{:x}", writer.hash.finalize()),
        });
        Ok(())
    }
}

impl Drop for CreatedOutput {
    fn drop(&mut self) {
        if !self.committed {
            for path in &self.created_files {
                let _ = fs::remove_file(path);
            }
            let _ = fs::remove_dir(&self.directory);
        }
    }
}

struct SampleRow<'a> {
    route: &'a crate::input::InputRoute,
    sample_kind: &'static str,
    nominal_offset_m: Option<f64>,
    station_m: f64,
    x_m: f64,
    y_m: f64,
    signed_offset_m: f64,
    elevation_m: Option<f64>,
    retention_reasons: String,
}

fn each_row<F>(results: &[RouteResult], mut visit: F) -> Result<(), SamplingFileError>
where
    F: FnMut(SampleRow<'_>) -> Result<(), SamplingFileError>,
{
    for route in results {
        for (index, profile) in std::iter::once(&route.result.centerline)
            .chain(&route.result.side_profiles)
            .enumerate()
        {
            for sample in &profile.samples {
                visit(SampleRow {
                    route: &route.route,
                    sample_kind: if index == 0 {
                        "centerline"
                    } else {
                        "side_profile"
                    },
                    nominal_offset_m: Some(profile.nominal_offset_m),
                    station_m: sample.station_m,
                    x_m: sample.x_m,
                    y_m: sample.y_m,
                    signed_offset_m: sample.signed_offset_m,
                    elevation_m: sample.elevation_m,
                    retention_reasons: serde_json::to_string(&sample.retention_reasons).unwrap(),
                })?;
            }
        }
        for (samples, sample_kind) in [
            (
                &route.result.scattered_observations,
                "scattered_observation",
            ),
            (&route.result.side_observations, "side_observation"),
        ] {
            for sample in samples {
                visit(SampleRow {
                    route: &route.route,
                    sample_kind,
                    nominal_offset_m: sample.nominal_side_offset_m,
                    station_m: sample.station_m,
                    x_m: sample.x_m,
                    y_m: sample.y_m,
                    signed_offset_m: sample.signed_offset_m,
                    elevation_m: sample.elevation_m,
                    retention_reasons: "[]".into(),
                })?;
            }
        }
    }
    Ok(())
}

fn write_csv(
    writer: &mut ArtifactWriter,
    results: &[RouteResult],
) -> Result<(), SamplingFileError> {
    let mut csv = csv::WriterBuilder::new().from_writer(writer);
    csv.write_record([
        "route_id",
        "feature_index",
        "part_index",
        "sample_kind",
        "row_class",
        "nominal_offset_m",
        "station_m",
        "x_m",
        "y_m",
        "signed_offset_m",
        "elevation_m",
        "coverage",
        "retention_reasons",
    ])
    .map_err(write_error)?;
    each_row(results, |row| {
        csv.write_record([
            row.route.route_id.clone(),
            row.route.feature_index.to_string(),
            row.route.part_index.to_string(),
            row.sample_kind.into(),
            DERIVED_ROW_CLASS.into(),
            row.nominal_offset_m
                .map(|v| v.to_string())
                .unwrap_or_default(),
            row.station_m.to_string(),
            row.x_m.to_string(),
            row.y_m.to_string(),
            row.signed_offset_m.to_string(),
            row.elevation_m.map(|v| v.to_string()).unwrap_or_default(),
            if row.elevation_m.is_some() {
                "supported"
            } else {
                "gap"
            }
            .into(),
            row.retention_reasons,
        ])
        .map_err(write_error)
    })?;
    csv.flush().map_err(write_error)
}

fn write_geojson(
    writer: &mut ArtifactWriter,
    results: &[RouteResult],
    projection: GridModelCrs,
) -> Result<(), SamplingFileError> {
    writer
        .write_all(b"{\"type\":\"FeatureCollection\",\"features\":[")
        .map_err(write_error)?;
    let mut first = true;
    each_row(results, |row| {
        if !first {
            writer.write_all(b",").map_err(write_error)?;
        }
        first = false;
        let xy = projection
            .to_map([row.x_m, row.y_m])
            .map_err(|error| SamplingFileError::SurfaceFailed(error.to_string()))?;
        // Unknown vertical datum is an attribute, never a GeoJSON ellipsoidal Z.
        let feature = json!({"type":"Feature", "geometry":{"type":"Point","coordinates":xy},
            "properties":{"route_id":row.route.route_id,"feature_id":row.route.feature_id,
                "feature_index":row.route.feature_index,"part_index":row.route.part_index,
                "sample_kind":row.sample_kind,"row_class":DERIVED_ROW_CLASS,
                "nominal_offset_m":row.nominal_offset_m,"station_m":row.station_m,
                "x_m":row.x_m,"y_m":row.y_m,"signed_offset_m":row.signed_offset_m,
                "elevation_m":row.elevation_m,"vertical_datum":"unknown",
                "coverage":if row.elevation_m.is_some(){"supported"}else{"gap"}}});
        serde_json::to_writer(&mut *writer, &feature).map_err(write_error)
    })?;
    writer.write_all(b"]}").map_err(write_error)
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn write_artifacts(
    out: &Path,
    request: &SamplingFileRequest,
    projection: GridModelCrs,
    inputs: &[InputEvidence],
    source: &Value,
    results: &[RouteResult],
    acquisition: &[FetchEvidence],
    survey_bytes: Option<&[u8]>,
    receipt: &mut SamplingFileReceipt,
) -> Result<(), SamplingFileError> {
    let mut created = CreatedOutput::new(out)?;
    created.artifact("results.json", |writer| {
        serde_json::to_writer_pretty(
            writer,
            &ResultsDocument {
                schema: "ds-terrain-sampling-results/v1",
                projected_crs: projection.canonical_definition(),
                source,
                derived_row_class: DERIVED_ROW_CLASS,
                routes: RouteViews(results),
            },
        )
        .map_err(write_error)
    })?;
    created.artifact("samples.csv", |writer| write_csv(writer, results))?;
    created.artifact("samples.geojson", |writer| {
        write_geojson(writer, results, projection)
    })?;
    if let Some(bytes) = survey_bytes {
        created.artifact("declared-survey-input.csv", |writer| {
            writer.write_all(bytes).map_err(write_error)
        })?;
    }
    let route_evidence: Vec<_> = results.iter().map(|route| {
        let (supported, missing) = profile_counts(&route.result);
        json!({"route_id":route.route.route_id,"feature_id":route.route.feature_id,
            "feature_index":route.route.feature_index,"part_index":route.route.part_index,
            "query_count":route.result.query_count,"route_length_m":route.result.route_length_m,
            "supported_profile_samples":supported,"missing_profile_samples":missing,
            "centerline_error_evidence":route.result.centerline.baseline_evidence,
            "side_error_evidence":route.result.side_profiles.iter().map(|profile| &profile.baseline_evidence).collect::<Vec<_>>(),
            "engineering_density":route.result.engineering_density})
    }).collect();
    let manifest = json!({
        "schema":"ds-terrain-sampling-manifest/v1", "request":request,
        "inputs":inputs,"route_input_crs":"EPSG:4326","projected_crs":projection.canonical_definition(),
        "projection_owner":"ds-geo::GridModelCrs","source":source,"settings":request.settings,
        "seed":request.settings.seed,"derived_row_class":DERIVED_ROW_CLASS,
        "source_observations_mutated":false,"source_measurements_independently_verified":false,
        "vertical_datum":"unknown","height_adjustment_m":0.0,"source_blending":false,
        "discrete_height_error_only":true,"continuous_error_bound":false,
        "density_and_accuracy_limitations":"density refers only to supplied engineering context; missing context, sampled source gaps and unknown datum/uncertainty limit engineering use; declared CSV authenticity is unverified",
        "coverage_boundary":"observed query gaps only, no inferred continuous coverage polygon",
        "acquisition":acquisition,"routes":route_evidence,
        "files":created.artifacts,"manifest_digest_location":"bounded invocation receipt"
    });
    created.artifact("manifest.json", |writer| {
        serde_json::to_writer_pretty(writer, &manifest).map_err(write_error)
    })?;
    receipt.files = created.artifacts.clone();
    check_receipt(receipt)?;
    created.committed = true;
    Ok(())
}
