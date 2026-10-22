use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use ds_geo::projection::GridModelCrs;
use ds_terrain_host::{
    DERIVED_ROW_CLASS, MAX_INPUT_BYTES, MAX_RECEIPT_BYTES, SamplingFileError, describe, sample_file,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

static NEXT_TEST: AtomicU64 = AtomicU64::new(0);

struct Fixture {
    directory: PathBuf,
    projection: GridModelCrs,
    origin: [f64; 2],
    request: Value,
}

impl Fixture {
    fn new() -> Self {
        let directory = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target/file-contract")
            .join(format!(
                "{}-{}",
                std::process::id(),
                NEXT_TEST.fetch_add(1, Ordering::Relaxed)
            ));
        fs::create_dir_all(&directory).unwrap();
        let projection = GridModelCrs::parse("EPSG:32735").unwrap();
        let origin = projection.from_map([30.0, -2.0]).unwrap();
        let fixture = Self {
            directory,
            projection,
            origin,
            request: json!({
                "route_file":"route.geojson","projected_crs":"EPSG:32735",
                "source":{"kind":"survey_tin","csv_file":"survey.csv","source_crs":"EPSG:32735",
                    "x_column":"E","y_column":"N","z_column":"H","max_triangle_edge_m":80.0},
                "settings":{"seed":8173,"maximum_gap_m":25.532,"station_jitter_m":5.333,
                    "baseline_step_m":1.5,"height_tolerance_m":0.1,"surface_uncertainty_m":0.2,
                    "side_offsets_m":[-6.0,6.0],"observation_lateral_jitter_m":1.5}
            }),
        };
        fixture.route(&[[0.0, 0.0], [50.0, 0.0], [100.0, 20.0]]);
        let mut csv = "E,N,H,note\n".to_string();
        for x in [-30.0, 0.0, 30.0, 60.0, 90.0, 120.0, 150.0] {
            for y in [-30.0, 0.0, 30.0] {
                csv.push_str(&format!(
                    "{},{},{},declared point\n",
                    origin[0] + x,
                    origin[1] + y,
                    height(x, y)
                ));
            }
        }
        fs::write(fixture.directory.join("survey.csv"), csv).unwrap();
        fixture
    }

    fn route(&self, metric_offsets: &[[f64; 2]]) {
        let coordinates: Vec<_> = metric_offsets
            .iter()
            .map(|point| {
                self.projection
                    .to_map([self.origin[0] + point[0], self.origin[1] + point[1]])
                    .unwrap()
            })
            .collect();
        fs::write(
            self.directory.join("route.geojson"),
            serde_json::to_vec(&json!({
                "type":"Feature","id":"survey-route","properties":{"name":"declared route"},
                "geometry":{"type":"LineString","coordinates":coordinates}
            }))
            .unwrap(),
        )
        .unwrap();
    }

    fn write_request(&self) -> PathBuf {
        let path = self.directory.join("request.json");
        fs::write(&path, serde_json::to_vec_pretty(&self.request).unwrap()).unwrap();
        path
    }

    fn output(&self, name: &str) -> PathBuf {
        self.directory.join(name)
    }

    fn run(
        &self,
        name: &str,
        dry_run: bool,
    ) -> Result<ds_terrain_host::SamplingFileReceipt, SamplingFileError> {
        sample_file(&self.write_request(), &self.output(name), dry_run)
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.directory);
    }
}

fn height(x: f64, y: f64) -> f64 {
    1600.0 + 0.2 * x + 0.5 * y
}
fn read_json(path: &Path) -> Value {
    serde_json::from_slice(&fs::read(path).unwrap()).unwrap()
}

#[test]
fn survey_plane_roundtrip_keeps_inputs_and_classifies_every_derived_row() {
    let fixture = Fixture::new();
    let before = fs::read(fixture.directory.join("survey.csv")).unwrap();
    let receipt = fixture.run("result", false).unwrap();
    assert_eq!(receipt.route_count, 1);
    assert_eq!(receipt.query_count, receipt.planned_query_count);
    assert!(!receipt.source_observations_mutated);
    assert!(!receipt.source_measurements_independently_verified);
    assert_eq!(receipt.source["vertical_datum"], "unknown");
    assert!(serde_json::to_vec(&receipt).unwrap().len() <= MAX_RECEIPT_BYTES);
    assert_eq!(
        fs::read(fixture.directory.join("survey.csv")).unwrap(),
        before
    );
    assert_eq!(
        fs::read(fixture.output("result/declared-survey-input.csv")).unwrap(),
        before
    );
    for artifact in &receipt.files {
        let bytes = fs::read(fixture.output("result").join(&artifact.file)).unwrap();
        assert_eq!(artifact.bytes, bytes.len() as u64);
        assert_eq!(artifact.sha256, format!("{:x}", Sha256::digest(&bytes)));
    }
    let results = read_json(&fixture.output("result/results.json"));
    let route = &results["routes"][0];
    assert_eq!(route["feature_id"], "survey-route");
    let result = &route["result"];
    assert_eq!(result["side_profiles"].as_array().unwrap().len(), 2);
    let profiles = std::iter::once(&result["centerline"])
        .chain(result["side_profiles"].as_array().unwrap().iter());
    for profile in profiles {
        assert_eq!(profile["baseline_evidence"]["continuous_bound"], false);
        for sample in profile["samples"].as_array().unwrap() {
            assert_eq!(sample["row_class"], DERIVED_ROW_CLASS);
            let expected = height(
                sample["x_m"].as_f64().unwrap() - fixture.origin[0],
                sample["y_m"].as_f64().unwrap() - fixture.origin[1],
            );
            assert!((sample["elevation_m"].as_f64().unwrap() - expected).abs() < 1e-7);
        }
    }
    for key in ["scattered_observations", "side_observations"] {
        for sample in result[key].as_array().unwrap() {
            assert_eq!(sample["row_class"], DERIVED_ROW_CLASS);
        }
    }
    let manifest = read_json(&fixture.output("result/manifest.json"));
    assert_eq!(
        manifest["settings"],
        serde_json::to_value(
            serde_json::from_value::<ds_terrain_sampling::SamplingOptions>(
                fixture.request["settings"].clone()
            )
            .unwrap()
        )
        .unwrap()
    );
    assert_eq!(manifest["source_blending"], false);
    assert_eq!(
        manifest["source_measurements_independently_verified"],
        false
    );
    assert!(manifest["acquisition"].as_array().unwrap().is_empty());
    let geojson = read_json(&fixture.output("result/samples.geojson"));
    for feature in geojson["features"].as_array().unwrap() {
        assert_eq!(feature["properties"]["row_class"], DERIVED_ROW_CLASS);
        assert_eq!(
            feature["geometry"]["coordinates"].as_array().unwrap().len(),
            2,
            "unknown vertical datum is never asserted as GeoJSON ellipsoidal Z"
        );
    }
    let mut reader = csv::Reader::from_path(fixture.output("result/samples.csv")).unwrap();
    let class_column = reader
        .headers()
        .unwrap()
        .iter()
        .position(|header| header == "row_class")
        .unwrap();
    assert!(
        reader
            .records()
            .all(|record| record.unwrap().get(class_column) == Some(DERIVED_ROW_CLASS))
    );
}

#[test]
fn outside_hull_queries_are_explicit_gaps_without_height_fallback() {
    let mut fixture = Fixture::new();
    fixture.route(&[[-20.0, 0.0], [50.0, 0.0], [180.0, 0.0]]);
    fixture.request["settings"]["side_offsets_m"] = json!([]);
    let receipt = fixture.run("gaps", false).unwrap();
    assert!(receipt.route_preview[0].missing_profile_samples.unwrap() > 0);
    let result = read_json(&fixture.output("gaps/results.json"));
    let center = &result["routes"][0]["result"]["centerline"];
    assert!(!center["gaps"].as_array().unwrap().is_empty());
    assert!(center["samples"].as_array().unwrap().last().unwrap()["elevation_m"].is_null());
    assert_eq!(
        center["samples"].as_array().unwrap().last().unwrap()["row_class"],
        DERIVED_ROW_CLASS
    );
}

#[test]
fn dry_run_validates_local_data_and_never_acquires_or_creates_output() {
    let mut fixture = Fixture::new();
    let receipt = fixture.run("dry-survey", true).unwrap();
    assert!(receipt.dry_run);
    assert!(receipt.planned_query_count > 0);
    assert_eq!(receipt.query_count, 0);
    assert!(receipt.files.is_empty());
    assert!(!fixture.output("dry-survey").exists());
    fixture.request["source"] = json!({"kind":"rwanda_tiff"});
    let receipt = fixture.run("dry-rwanda", true).unwrap();
    assert_eq!(receipt.source["id"], "rwanda_dem_10m");
    assert!(!fixture.output("dry-rwanda").exists());
}

#[test]
fn invalid_request_and_breaklines_refuse_before_provider_or_source_file_access() {
    let mut fixture = Fixture::new();
    fixture.request["source"] = json!({"kind":"rwanda_tiff"});
    fixture.request["settings"]["maximum_gap_m"] = json!(-1.0);
    assert_eq!(
        fixture.run("invalid-provider", false).unwrap_err().code(),
        "terrain_sampling_failed"
    );
    assert!(!fixture.output("invalid-provider").exists());
    fixture.request["settings"]["maximum_gap_m"] = json!(25.0);
    fixture.request["source"]["unexpected"] = json!(true);
    assert_eq!(
        fixture.run("unknown", false).unwrap_err().code(),
        "terrain_request_invalid"
    );
    fixture.request["source"] = json!({"kind":"survey_tin","csv_file":"missing.csv",
        "source_crs":"EPSG:32735","x_column":"E","y_column":"N","z_column":"H",
        "max_triangle_edge_m":20.0,"breaklines":[{"vertices":[{"x":0,"y":0,"z":0},{"x":1,"y":1,"z":1}]}]});
    fixture.request["route_file"] = json!("missing-route.geojson");
    assert_eq!(
        fixture.run("breaklines", false).unwrap_err().code(),
        "terrain_breaklines_unsupported"
    );
    assert!(!fixture.output("breaklines").exists());
}

#[test]
fn output_collision_keeps_existing_bytes_and_includes_dry_run() {
    let fixture = Fixture::new();
    fs::create_dir(fixture.output("existing")).unwrap();
    fs::write(fixture.output("existing/owner.txt"), b"owner bytes").unwrap();
    for dry in [true, false] {
        assert_eq!(
            fixture.run("existing", dry).unwrap_err().code(),
            "terrain_output_exists"
        );
    }
    assert_eq!(
        fs::read(fixture.output("existing/owner.txt")).unwrap(),
        b"owner bytes"
    );
    let request = fixture.write_request();
    let before = fs::read(&request).unwrap();
    assert_eq!(
        sample_file(&request, &request, false).unwrap_err().code(),
        "terrain_output_exists"
    );
    assert_eq!(fs::read(&request).unwrap(), before);
}

#[test]
fn declared_survey_conflicts_and_byte_limit_refuse_without_output() {
    let fixture = Fixture::new();
    let mut bytes = fs::read(fixture.directory.join("survey.csv")).unwrap();
    bytes.extend_from_slice(
        format!(
            "{},{},9999,conflicting declared point\n",
            fixture.origin[0], fixture.origin[1]
        )
        .as_bytes(),
    );
    fs::write(fixture.directory.join("survey.csv"), bytes).unwrap();
    assert_eq!(
        fixture.run("conflict", false).unwrap_err().code(),
        "terrain_source_invalid"
    );
    assert!(!fixture.output("conflict").exists());
    let file = fs::File::create(fixture.directory.join("survey.csv")).unwrap();
    file.set_len(MAX_INPUT_BYTES + 1).unwrap();
    assert_eq!(
        fixture.run("too-large", true).unwrap_err().code(),
        "terrain_input_too_large"
    );
    assert!(!fixture.output("too-large").exists());
}

#[test]
fn multiple_parts_preserve_numeric_ids_and_bound_only_the_inline_preview() {
    let mut fixture = Fixture::new();
    fixture.request["settings"]["side_offsets_m"] = json!([]);
    let parts: Vec<_> = (0..6)
        .map(|index| {
            [[0.0, index as f64], [50.0, index as f64]].map(|point| {
                fixture
                    .projection
                    .to_map([fixture.origin[0] + point[0], fixture.origin[1] + point[1]])
                    .unwrap()
            })
        })
        .collect();
    fs::write(
        fixture.directory.join("route.geojson"),
        serde_json::to_vec(&json!({
            "type":"FeatureCollection","features":[{"type":"Feature","id":31,"properties":{},
                "geometry":{"type":"MultiLineString","coordinates":parts}}]
        }))
        .unwrap(),
    )
    .unwrap();
    let receipt = fixture.run("multi", false).unwrap();
    assert_eq!(receipt.route_count, 6);
    assert_eq!(receipt.route_preview.len(), 5);
    assert_eq!(receipt.more_routes, 1);
    let results = read_json(&fixture.output("multi/results.json"));
    for (part, route) in results["routes"].as_array().unwrap().iter().enumerate() {
        assert_eq!(route["route_id"], "31");
        assert_eq!(route["feature_id"], 31);
        assert_eq!(route["part_index"], part);
    }
}

#[test]
fn same_seed_is_reproducible_and_wgs84_survey_uses_native_projection() {
    let mut fixture = Fixture::new();
    fixture.request["source"]["source_crs"] = json!("EPSG:4326");
    let mut csv = "E,N,H,note\n".to_string();
    for x in [-30.0, 0.0, 30.0, 60.0, 90.0, 120.0, 150.0] {
        for y in [-30.0, 0.0, 30.0] {
            let wgs84 = fixture
                .projection
                .to_map([fixture.origin[0] + x, fixture.origin[1] + y])
                .unwrap();
            csv.push_str(&format!(
                "{},{},{},declared WGS84\n",
                wgs84[0],
                wgs84[1],
                height(x, y)
            ));
        }
    }
    fs::write(fixture.directory.join("survey.csv"), csv).unwrap();
    fixture.run("first", false).unwrap();
    fixture.run("second", false).unwrap();
    for filename in ["results.json", "samples.csv", "samples.geojson"] {
        assert_eq!(
            fs::read(fixture.output("first").join(filename)).unwrap(),
            fs::read(fixture.output("second").join(filename)).unwrap()
        );
    }
}

#[test]
fn descriptor_is_generated_strict_schema_with_actual_defaults_and_semantics() {
    let descriptor = describe();
    assert_eq!(descriptor["request_schema"]["additionalProperties"], false);
    assert_eq!(
        descriptor["defaults"]["settings"]["side_offsets_m"],
        json!([])
    );
    assert_eq!(
        descriptor["defaults"]["settings"],
        serde_json::to_value(ds_terrain_sampling::SamplingOptions::default()).unwrap()
    );
    assert_eq!(descriptor["limits"]["survey_points"], 6000);
    assert_eq!(descriptor["limits"]["aggregate_surface_queries"], 1_000_000);
    assert!(
        descriptor["semantics"]["accuracy"]
            .as_str()
            .unwrap()
            .contains("discrete")
    );
    let required = descriptor["request_schema"]["required"].as_array().unwrap();
    for field in ["route_file", "projected_crs", "source"] {
        assert!(required.contains(&json!(field)));
    }
}
