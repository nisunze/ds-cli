use super::*;
use crate::{EngineExit, HostFailure};
use sha2::{Digest, Sha256};
use std::sync::Mutex;

fn receipt(country: &str) -> InputReceipt {
    let sheets = r#"{"project_settings":[{"parameter":"report_formats","value":"gpkg"}]}"#;
    InputReceipt {
        local_print_recipe: None,
        schema: 1,
        country: country.into(),
        sheets_json: sheets.into(),
        sheets_sha256: format!("{:x}", Sha256::digest(sheets.as_bytes())),
        reference_semantic_sha256: "7".repeat(64),
    }
}

fn room(features: usize) -> BTreeMap<String, Value> {
    let feature = json!({"type":"Feature","properties":{"pole_number":"P1"},"geometry":{"type":"Point","coordinates":[30.06,-1.94]}});
    serde_json::from_value(json!({
        "lv_poles": {"type": "FeatureCollection", "features": vec![feature; features]},
        "voltage_drop": {"type": "FeatureCollection", "features": []},
    }))
    .unwrap()
}

fn build_info() -> Value {
    json!({
        "schema": "ds.engine-build/v1", "service": "ds-network-reporter", "binary": "ds-report",
        "package_version": "0.1.0", "source_sha": "a".repeat(40),
        "engine_version": format!("ds-network-reporter@0.1.0+{}", "a".repeat(40)),
        "cargo_lock_sha256": "d".repeat(64), "features": [], "rustc": "rustc 1.95.0",
        "target": "x86_64-unknown-linux-gnu", "profile": "release",
        "schemas": ["ds.engine-build/v1"], "build_manifest_sha256": "f".repeat(64)
    })
}

/// Answers like `ds-report`: reads the request, writes every requested
/// artifact into `out_dir` and writes its result document.
#[derive(Default)]
struct FakeCombined {
    behaviour: &'static str,
    requests: Mutex<Vec<Value>>,
    media: Mutex<Option<Value>>,
}

impl ReportEngine for FakeCombined {
    fn build_info(&self) -> Result<Value, HostFailure> {
        Ok(build_info())
    }
    fn export_transformer_report(&self, _: &Path, _: &Path) -> Result<EngineExit, HostFailure> {
        unreachable!("a combined run never exports one transformer")
    }
    fn discover_combined_media_references(
        &self,
        request: &Path,
        result: &Path,
    ) -> Result<EngineExit, HostFailure> {
        let request: Value = serde_json::from_slice(&fs::read(request).unwrap()).unwrap();
        let rooms = request["transformers"].as_array().unwrap().len();
        *self.media.lock().unwrap() = Some(request);
        fs::write(result, serde_json::to_vec(&json!({"schema_version":2,"task":decisions::MEDIA_TASK,"status":"completed","engine":build_info(),"reference_count":2,"source_projects":["project-a"],"transformers_scanned":rooms,"features_scanned":3,"total_input_bytes":1,"elapsed_ms":1})).unwrap()).unwrap();
        Ok(EngineExit {
            succeeded: true,
            summary: String::new(),
        })
    }
    fn export_combined_transformer_report(
        &self,
        request: &Path,
        result: &Path,
    ) -> Result<EngineExit, HostFailure> {
        let request: Value = serde_json::from_slice(&fs::read(request).unwrap()).unwrap();
        self.requests.lock().unwrap().push(request.clone());
        if self.behaviour == "refuse" {
            return Ok(EngineExit {
                succeeded: false,
                summary: "combined_media_requires_supplied_links: grant needed".into(),
            });
        }
        let out = PathBuf::from(request["out_dir"].as_str().unwrap());
        let file = |path: &str| {
            let bytes = fs::read(path).unwrap();
            json!({"bytes": bytes.len(), "sha256": format!("{:x}", Sha256::digest(&bytes))})
        };
        let rooms: Vec<Value> = request["transformers"].as_array().unwrap().iter().map(|room| {
                json!({"transformer": room["transformer"], "source": file(room["layers"].as_str().unwrap()), "layers_decoded": 1, "features_decoded": 1, "geometry_changed": false, "admin_features_requested": 0, "admin_features_matched": 0})
            }).collect();
        let mut total = file(request["network_config"].as_str().unwrap())["bytes"]
            .as_u64()
            .unwrap();
        total += rooms
            .iter()
            .map(|room| room["source"]["bytes"].as_u64().unwrap())
            .sum::<u64>();
        let media = request
            .get("media_grant")
            .map(|path| file(path.as_str().unwrap()));
        total += media
            .as_ref()
            .map_or(0, |receipt| receipt["bytes"].as_u64().unwrap());
        let mut artifacts = Vec::new();
        let formats: Vec<String> = serde_json::from_value(request["formats"].clone()).unwrap();
        for format in &formats {
            let filename =
                ds_command_kernel::report_formats::report_filename(decisions::IDENTITY, format)
                    .unwrap();
            let path = out.join(&filename);
            fs::write(&path, format!("{format} bytes")).unwrap();
            if self.behaviour == "tamper" && format == "xlsx" {
                fs::write(&path, "tampered").unwrap();
                artifacts.push(json!({"format": format, "filename": filename, "path": path, "bytes": 10, "sha256": format!("{:x}", Sha256::digest(b"xlsx bytes"))}));
                continue;
            }
            let bytes = fs::read(&path).unwrap();
            artifacts.push(json!({"format": format, "filename": filename, "path": path, "bytes": bytes.len(), "sha256": format!("{:x}", Sha256::digest(&bytes))}));
        }
        let mut status = "completed";
        let mut blockers = json!([]);
        if self.behaviour == "partial" {
            status = "partial";
            let removed = artifacts.pop().unwrap();
            blockers = json!([{"code": "artifact_stage_failed", "message": "disk full", "format": removed["format"]}]);
        }
        let document = json!({
            "schema_version": 4, "task": decisions::TASK, "status": status, "engine": build_info(),
            "prefix": decisions::IDENTITY, "country": request["country"],
            "upstream_admission_requirements": [{"code": decisions::REVIEWS_REQUIREMENT, "signed_work_grant_field": "transformer_reviews_absent", "required_value": true, "must_bind_input_fingerprint": true, "forwarded_to_task": false, "message": "m"}],
            "input_receipt": {"network_config": file(request["network_config"].as_str().unwrap()), "media_grant": media, "transformers": rooms, "total_input_bytes": total},
            "layers_aggregated": 1, "features_aggregated": 2, "geometry_changed": false,
            "admin_bounds": {"configured": false, "villages_loaded": 0, "features_matched": 0, "features_requested": 0},
            "solar": {"configured": false}, "formats_requested": formats, "artifacts": artifacts,
            "tiles": {}, "blockers": blockers,
            "phase_timings": {"input_preparation_ms": 1, "format_compute_ms": 2, "artifact_finalize_ms": 3}, "elapsed_ms": 6
        });
        fs::write(result, serde_json::to_vec(&document).unwrap()).unwrap();
        Ok(EngineExit {
            succeeded: status != "failed",
            summary: String::new(),
        })
    }
}

fn begin(root: &Path, receipt: &InputReceipt) -> CombinedStaging {
    CombinedStaging::begin(&CombinedSettings {
        project_id: "project-a",
        receipt,
        admin_bounds: None,
        staging_root: &root.join("staging"),
        out_root: &root.join("out"),
    })
    .unwrap()
}

#[test]
fn rooms_stage_one_at_a_time_and_the_run_places_the_verified_package() {
    let root = tempfile::tempdir().unwrap();
    let receipt = receipt("Burundi");
    let mut staging = begin(root.path(), &receipt);
    assert!(staging.add_room("tx_b", 2, None, &room(2)).unwrap());
    assert!(!staging.add_room("tx_empty", 1, None, &room(0)).unwrap());
    assert!(staging.add_room("tx_a", 5, None, &room(1)).unwrap());
    assert!(staging.add_room("tx_a", 5, None, &room(1)).is_err());
    assert_eq!(staging.empty_rooms(), ["tx_empty"]);
    let engine = FakeCombined::default();
    let scope = staging.media_scope(&engine).unwrap();
    assert_eq!(scope.reference_count, 2);
    let media = engine.media.lock().unwrap().clone().unwrap();
    assert_eq!(media["transformers"][0]["transformer"], "tx_a");
    let natures = BTreeMap::from([("tx_a".to_string(), "fill_in".to_string())]);
    let grant = br#"{"grants":[]}"#;
    let staging_root = root.path().join("staging");
    let outcome = staging
        .run(
            &engine,
            &CombinedExtras {
                media_grant: Some(grant),
                transformer_natures: Some(&natures),
            },
        )
        .unwrap();
    let request = engine.requests.lock().unwrap()[0].clone();
    assert_eq!(
        request["formats"],
        json!(["shp", "xlsx", "geojsonseq", "gpkg"])
    );
    assert_eq!(request["transformer_natures"]["tx_a"], "fill_in");
    assert!(request.get("admin_bounds_asset").is_none());
    // Staging is gone once the run ends.
    assert!(!Path::new(request["transformers"][0]["layers"].as_str().unwrap()).exists());
    assert_eq!(fs::read_dir(&staging_root).unwrap().count(), 0);
    let placed = root.path().join("out").join("combined_transformer");
    for artifact in &outcome.run.artifacts {
        assert!(
            placed.join(&artifact.filename).is_file(),
            "{}",
            artifact.filename
        );
    }
    assert_eq!(outcome.run.transformer, "combined_transformer");
    assert_eq!(outcome.run.receipt["transformer_revision"], 0);
    assert_eq!(
        outcome.run.receipt["input_base_fingerprint"],
        json!(outcome.input_base_fingerprint)
    );
    assert_eq!(
        outcome
            .pins
            .iter()
            .map(|pin| pin.transformer.as_str())
            .collect::<Vec<_>>(),
        ["tx_a", "tx_b"]
    );
    assert!(placed.join(RUN_RECEIPT_FILE).is_file());
}

#[test]
fn partial_tampered_or_refused_runs_leave_nothing_under_the_output_root() {
    for behaviour in ["partial", "tamper", "refuse"] {
        let root = tempfile::tempdir().unwrap();
        let receipt = receipt("Burundi");
        let mut staging = begin(root.path(), &receipt);
        staging.add_room("tx_a", 1, None, &room(1)).unwrap();
        let engine = FakeCombined {
            behaviour,
            ..Default::default()
        };
        let error = staging
            .run(&engine, &CombinedExtras::default())
            .unwrap_err();
        let expected = match behaviour {
            "partial" => CODE_EXPORT_BLOCKED,
            "tamper" => CODE_RESULT_INVALID,
            _ => CODE_ENGINE_REFUSED,
        };
        assert_eq!(error.code, expected, "{behaviour}: {error}");
        assert!(
            !root
                .path()
                .join("out")
                .join("combined_transformer")
                .exists(),
            "{behaviour}"
        );
        assert_eq!(
            fs::read_dir(root.path().join("staging")).unwrap().count(),
            0,
            "{behaviour}"
        );
    }
}

#[test]
fn rwanda_needs_the_reference_and_an_existing_package_is_never_overwritten() {
    let root = tempfile::tempdir().unwrap();
    let rwanda = receipt("Rwanda");
    let settings = CombinedSettings {
        project_id: "project-a",
        receipt: &rwanda,
        admin_bounds: None,
        staging_root: &root.path().join("staging"),
        out_root: &root.path().join("out"),
    };
    assert_eq!(
        CombinedStaging::begin(&settings).err().unwrap().code,
        CODE_ADMIN_BOUNDS_UNAVAILABLE
    );
    let other = receipt("Burundi");
    fs::create_dir_all(root.path().join("out").join("combined_transformer")).unwrap();
    let existing = CombinedSettings {
        receipt: &other,
        ..settings
    };
    assert_eq!(
        CombinedStaging::begin(&existing).err().unwrap().code,
        CODE_OUTPUT_EXISTS
    );
    let mut staging = CombinedStaging::begin(&CombinedSettings {
        out_root: &root.path().join("fresh"),
        ..existing
    })
    .unwrap();
    let digest = "0".repeat(64);
    assert_eq!(
        staging
            .add_room("tx_a", 1, Some(&digest), &room(1))
            .unwrap_err()
            .code,
        CODE_INPUTS_INVALID
    );
    assert_eq!(
        staging
            .add_room("collisions", 1, None, &room(1))
            .unwrap_err()
            .code,
        CODE_INPUTS_INVALID
    );
}
