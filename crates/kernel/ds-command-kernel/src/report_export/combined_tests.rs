use super::*;
use crate::report_export::INPUT_RECEIPT_SCHEMA;

fn receipt(country: &str, settings: Value) -> InputReceipt {
    let sheets_json = json!({"project_settings": settings}).to_string();
    InputReceipt {
        local_print_recipe: None,
        schema: INPUT_RECEIPT_SCHEMA,
        country: country.into(),
        sheets_sha256: sha256_hex(sheets_json.as_bytes()),
        sheets_json,
        reference_semantic_sha256: "b".repeat(64),
    }
}

fn layers(features: usize) -> BTreeMap<String, Value> {
    let feature = json!({"type": "Feature", "geometry": {"type": "Point", "coordinates": [30.0, -2.0]}, "properties": {"id": "p"}});
    serde_json::from_value(json!({
            "lv_poles": {"type": "FeatureCollection", "features": vec![feature; features]},
            "drafting_errors": {"type": "FeatureCollection", "features": [{"type": "Feature", "geometry": null, "properties": {}}]},
        }))
        .unwrap()
}

fn staged_room(name: &str) -> StagedRoom {
    stage_room(name, 3, None, &layers(2)).unwrap().unwrap()
}

#[test]
fn the_plan_is_the_shared_data_only_combined_plan_and_names_no_print() {
    let scope = scope(
        "project-a",
        &receipt(
            "Rwanda",
            json!([{"parameter": "report_formats", "value": "pdf_a3, gpkg, voltage_drop"}]),
        ),
    )
    .unwrap();
    assert_eq!(scope.formats(), ["shp", "xlsx", "geojsonseq", "gpkg"]);
    assert!(scope.requires_admin_bounds);
    let plain = super::scope("project-a", &receipt("Burundi", json!([]))).unwrap();
    assert_eq!(plain.formats(), ["shp", "xlsx", "geojsonseq"]);
    assert!(!plain.requires_admin_bounds);
    assert!(super::scope("bad project", &receipt("Burundi", json!([]))).is_err());
}

#[test]
fn a_room_stages_only_report_layers_and_pins_its_saved_digest() {
    let room = layers(2);
    let digest = jcs::layers_content_digest(&room).unwrap();
    let staged = stage_room("tx_a", 4, Some(&digest), &room)
        .unwrap()
        .unwrap();
    let document: Value = serde_json::from_slice(&staged.document).unwrap();
    assert_eq!(
        document["layers"]
            .as_object()
            .unwrap()
            .keys()
            .collect::<Vec<_>>(),
        ["lv_poles"]
    );
    assert_eq!(staged.pin.content_digest, digest);
    assert_eq!(staged.pin.features, 2);
    assert_eq!(staged.pin.source_sha256, sha256_hex(&staged.document));
    assert!(stage_room("tx_a", 4, Some(&"0".repeat(64)), &room)
        .unwrap_err()
        .contains("saved content digest"));
    assert!(stage_room("tx_a", 0, None, &room).is_err());
    for reserved in ["combined_transformer", "collisions", "mv_data"] {
        assert!(stage_room(reserved, 1, None, &room).is_err(), "{reserved}");
    }
    assert_eq!(stage_room("tx_a", 4, None, &layers(0)).unwrap(), None);
}

#[test]
fn pins_sort_as_the_reporter_sorts_and_refuse_repeats_and_empty_runs() {
    let pins = vec![staged_room("tx_b").pin, staged_room("tx_a").pin];
    let sorted = sorted_pins(pins.clone()).unwrap();
    assert_eq!(sorted[0].transformer, "tx_a");
    assert!(sorted_pins(Vec::new()).is_err());
    assert!(sorted_pins(vec![pins[0].clone(), pins[0].clone()]).is_err());
    let first = input_base_fingerprint(
        &super::scope("p", &receipt("Burundi", json!([]))).unwrap(),
        &sorted,
        None,
    )
    .unwrap();
    let mut changed = sorted.clone();
    changed[0].revision += 1;
    let second = input_base_fingerprint(
        &super::scope("p", &receipt("Burundi", json!([]))).unwrap(),
        &changed,
        None,
    )
    .unwrap();
    assert_ne!(first, second);
    assert_ne!(
        room_content_sha256(&sorted).unwrap(),
        room_content_sha256(&changed).unwrap()
    );
}

struct Fixture {
    scope: Scope,
    rooms: Vec<(PathBuf, RoomPin)>,
    config: FileReceipt,
    reference: FileReceipt,
    out: PathBuf,
    engine: EngineIdentity,
}

impl Fixture {
    fn new(country: &str) -> Self {
        let scope = scope("project-a", &receipt(country, json!([]))).unwrap();
        let rooms = sorted_pins(vec![staged_room("tx_b").pin, staged_room("tx_a").pin])
            .unwrap()
            .into_iter()
            .enumerate()
            .map(|(index, pin)| (PathBuf::from(format!("/stage/rooms/{index}.json")), pin))
            .collect();
        Self {
            scope,
            rooms,
            config: FileReceipt::of(b"{}"),
            reference: FileReceipt {
                bytes: 10,
                sha256: "c".repeat(64),
            },
            out: PathBuf::from("/stage/out"),
            engine: EngineIdentity {
                engine_version: format!("ds-network-reporter@0.1.0+{}", "e".repeat(40)),
                build_manifest_sha256: "d".repeat(64),
                package_version: "0.1.0".into(),
                source_sha: "e".repeat(40),
                profile: "release".into(),
            },
        }
    }
    fn staged(&self) -> Staged<'_> {
        Staged {
            network_config: (Path::new("/stage/config.json"), &self.config),
            rooms: &self.rooms,
            out_dir: &self.out,
            admin_bounds: self
                .scope
                .requires_admin_bounds
                .then(|| (Path::new("/shared/asset.dsab"), &self.reference)),
            media_grant: None,
            transformer_natures: None,
        }
    }
    fn result(&self) -> Value {
        let rwanda = self.scope.requires_admin_bounds;
        let total = self.config.bytes
            + if rwanda { self.reference.bytes } else { 0 }
            + self
                .rooms
                .iter()
                .map(|(_, pin)| pin.source_bytes)
                .sum::<u64>();
        json!({
            "engineering_preflight": {"blockers": 2},
            "schema_version": 4, "task": TASK, "status": "completed",
            "engine": {"schema": "ds.engine-build/v1", "service": "ds-network-reporter", "binary": "ds-report",
                "package_version": "0.1.0", "source_sha": "e".repeat(40), "engine_version": self.engine.engine_version,
                "cargo_lock_sha256": "f".repeat(64), "features": [], "rustc": "rustc", "target": "x86_64", "profile": "release",
                "schemas": ["s"], "build_manifest_sha256": "d".repeat(64)},
            "prefix": IDENTITY, "country": self.scope.country,
            "upstream_admission_requirements": [{"code": REVIEWS_REQUIREMENT, "signed_work_grant_field": "transformer_reviews_absent", "required_value": true, "must_bind_input_fingerprint": true, "forwarded_to_task": false, "message": "m"}],
            "input_receipt": {
                "network_config": self.config,
                "admin_bounds_asset": if rwanda { json!(self.reference) } else { Value::Null },
                "transformers": self.rooms.iter().map(|(_, pin)| json!({"transformer": pin.transformer, "source": {"bytes": pin.source_bytes, "sha256": pin.source_sha256}, "layers_decoded": 1, "features_decoded": 2, "geometry_changed": false, "admin_features_requested": 2, "admin_features_matched": 2})).collect::<Vec<_>>(),
                "total_input_bytes": total,
            },
            "layers_aggregated": 2, "features_aggregated": 4, "geometry_changed": false,
            "admin_bounds": {"configured": rwanda, "villages_loaded": if rwanda { 5 } else { 0 }, "features_matched": if rwanda { 4 } else { 0 }, "features_requested": if rwanda { 4 } else { 0 }},
            "solar": {"configured": false},
            "formats_requested": self.scope.formats(),
            "artifacts": self.scope.outputs.iter().map(|output| {
                let filename = report_formats::report_filename(IDENTITY, &output.output_id).unwrap();
                json!({"format": output.format, "filename": filename, "path": self.out.join(&filename), "bytes": 9, "sha256": "a".repeat(64)})
            }).collect::<Vec<_>>(),
            "tiles": {"status": "ready_for_background"},
            "blockers": [],
            "phase_timings": {"input_preparation_ms": 1, "format_compute_ms": 2, "artifact_finalize_ms": 3},
            "elapsed_ms": 7,
        })
    }
    fn verify(&self, result: &Value) -> Result<VerifiedCombined, CombinedRefusal> {
        verify_result(
            &serde_json::to_vec(result).unwrap(),
            &self.scope,
            &self.staged(),
            &self.engine,
        )
    }
}

#[test]
fn the_request_names_every_staged_input_and_only_this_plan() {
    let fixture = Fixture::new("Rwanda");
    let request = engine_request(&fixture.scope, &fixture.staged()).unwrap();
    assert_eq!(request["formats"], json!(["shp", "xlsx", "geojsonseq"]));
    assert_eq!(request["transformers"][0]["transformer"], "tx_a");
    assert_eq!(request["admin_bounds_sha256"], "c".repeat(64));
    assert!(request.get("media_grant").is_none());
    let media = media_request(&fixture.scope, &fixture.staged()).unwrap();
    assert_eq!(media["transformers"], request["transformers"]);
    let mut unsorted = fixture.rooms.clone();
    unsorted.reverse();
    let staged = Staged {
        rooms: &unsorted,
        ..fixture.staged()
    };
    assert!(engine_request(&fixture.scope, &staged).is_err());
    let missing_reference = Staged {
        admin_bounds: None,
        ..fixture.staged()
    };
    assert!(engine_request(&fixture.scope, &missing_reference).is_err());
}

#[test]
fn a_complete_answer_verifies_every_planned_artifact_and_receipt() {
    for country in ["Rwanda", "Burundi"] {
        let fixture = Fixture::new(country);
        let verified = fixture.verify(&fixture.result()).unwrap();
        assert_eq!(
            verified
                .artifacts
                .iter()
                .map(|a| a.filename.as_str())
                .collect::<Vec<_>>(),
            [
                "combined_transformer.shp.zip",
                "combined_transformer.xlsx",
                "combined_transformer_geojsonl.zip"
            ]
        );
        assert_eq!(verified.engineering_blockers, 2);
        let receipt = run_receipt(&ReceiptFacts {
            scope: &fixture.scope,
            pins: &fixture
                .rooms
                .iter()
                .map(|(_, pin)| pin.clone())
                .collect::<Vec<_>>(),
            client_run_id: &format!("report-{}", "1".repeat(32)),
            engine: &fixture.engine,
            result: &verified,
            artifact_dir: IDENTITY,
            now_ms: 1,
            transformer_natures: None,
            media_grant_sha256: None,
        })
        .unwrap();
        assert_eq!(receipt["transformer"], IDENTITY);
        assert_eq!(receipt["transformer_revision"], 0);
        assert_eq!(receipt["project_id"], "project-a");
        assert!(exact_lower_sha256(
            receipt["input_base_fingerprint"].as_str().unwrap()
        ));
        assert_eq!(receipt["transformer_reviews"], "not_included");
        assert_eq!(
            receipt["engineering_findings"], 2,
            "a complete set counts the findings its workbook itemizes"
        );
        assert_eq!(receipt["status"], "completed");
    }
}

#[test]
fn partial_crossed_or_unplanned_answers_never_become_a_deliverable() {
    let fixture = Fixture::new("Rwanda");
    let mut partial = fixture.result();
    partial["status"] = json!("partial");
    partial["artifacts"].as_array_mut().unwrap().pop();
    partial["blockers"] =
        json!([{"code": "artifact_stage_failed", "message": "disk", "format": "geojsonseq"}]);
    assert!(matches!(
        fixture.verify(&partial),
        Err(CombinedRefusal::Incomplete { artifacts: 2, .. })
    ));
    for corrupt in [
        ("country", json!("Burundi")),
        ("prefix", json!("tx_a")),
        ("formats_requested", json!(["shp", "xlsx"])),
        ("upstream_admission_requirements", json!([])),
        ("solar", json!({"configured": true})),
        ("layers_aggregated", json!(0)),
    ] {
        let mut result = fixture.result();
        result[corrupt.0] = corrupt.1.clone();
        assert!(
            matches!(
                fixture.verify(&result),
                Err(CombinedRefusal::Invalid { .. })
            ),
            "{}",
            corrupt.0
        );
    }
    let mut crossed_room = fixture.result();
    crossed_room["input_receipt"]["transformers"][0]["source"]["sha256"] = json!("0".repeat(64));
    assert!(fixture.verify(&crossed_room).is_err());
    let mut crossed_total = fixture.result();
    crossed_total["input_receipt"]["total_input_bytes"] = json!(1);
    assert!(fixture.verify(&crossed_total).is_err());
    let mut moved = fixture.result();
    moved["artifacts"][0]["path"] = json!("/elsewhere/combined_transformer.shp.zip");
    assert!(fixture.verify(&moved).is_err());
    let mut other_engine = fixture.result();
    other_engine["engine"]["build_manifest_sha256"] = json!("9".repeat(64));
    assert!(fixture.verify(&other_engine).is_err());
    let mut print = fixture.result();
    print["artifacts"][0]["paper_size"] = json!("A3");
    assert!(fixture.verify(&print).is_err());
}

#[test]
fn a_refused_workbook_names_the_engineering_facts_per_transformer() {
    let fixture = Fixture::new("Burundi");
    let mut partial = fixture.result();
    partial["status"] = json!("partial");
    partial["artifacts"]
        .as_array_mut()
        .unwrap()
        .retain(|artifact| artifact["format"] != "xlsx");
    partial["blockers"] = json!([{"code": "report_engineering_facts_required", "message": "4 missing facts", "format": "xlsx"}]);
    let finding = |transformer: &str, layer: &str, field: &str| json!({"code": "engineering_attribute_missing", "layer": layer, "field": field, "transformer": transformer, "feature_id": "n1", "feature_index": 0, "recorded": null});
    // A project-wide preflight holds more source rows than one
    // transformer's bounds admit; it is still folded.
    let source_layers: Vec<Value> = (0..40)
            .map(|index| json!({"layer": format!("lv_poles_{index}"), "features": 1, "held_features_sha256": "a".repeat(64)}))
            .collect();
    partial["engineering_preflight"] = json!({
        "schema": "ds.report.engineering-preflight/v1", "source_layers": source_layers, "field_lineage": [],
        "blockers": 4, "finding_counts": {"engineering_attribute_missing": 4}, "coverage_counts": {},
        "findings": [
            finding("TX_B", "lv_poles", "struct_type"),
            finding("TX_B", "lv_poles", "stay"),
            finding("TX_A", "tapping_poles", "assembly_type"),
        ],
        "omitted_details": 1, "quantity_rows_total": 0, "quantity_lineage": [], "omitted_quantity_details": 0,
    });
    let Err(CombinedRefusal::Incomplete {
        engineering: Some(gaps),
        artifacts,
        ..
    }) = fixture.verify(&partial)
    else {
        panic!("a refused workbook names its engineering gaps");
    };
    assert_eq!(artifacts, 2);
    assert_eq!((gaps.blockers, gaps.omitted_details), (4, 1));
    assert_eq!(
        gaps.transformers,
        [
            EngineeringGap {
                transformer: "TX_A".into(),
                findings: 1,
                fields: vec!["tapping_poles.assembly_type".into()],
            },
            EngineeringGap {
                transformer: "TX_B".into(),
                findings: 2,
                fields: vec!["lv_poles.stay".into(), "lv_poles.struct_type".into()],
            },
        ]
    );
    // A preflight outside the closed document names nothing rather than guessing.
    partial["engineering_preflight"]["blockers"] = json!(5);
    assert!(matches!(
        fixture.verify(&partial),
        Err(CombinedRefusal::Incomplete {
            engineering: None,
            ..
        })
    ));
}

#[test]
fn media_scope_answers_the_staged_rooms_and_names_foreign_projects() {
    let document = |value: Value| serde_json::to_vec(&value).unwrap();
    let answer = json!({"schema_version": 2, "task": MEDIA_TASK, "status": "completed", "reference_count": 3, "source_projects": ["other", "project-a"], "transformers_scanned": 2, "features_scanned": 9, "total_input_bytes": 1, "elapsed_ms": 1});
    let scope = verify_media_scope(&document(answer.clone()), 2).unwrap();
    assert_eq!(scope.foreign_projects("project-a"), ["other"]);
    let mut wrong = answer.clone();
    wrong["transformers_scanned"] = json!(1);
    assert!(verify_media_scope(&document(wrong), 2).is_err());
    let mut unsorted = answer.clone();
    unsorted["source_projects"] = json!(["project-a", "other"]);
    assert!(verify_media_scope(&document(unsorted), 2).is_err());
    let mut inconsistent = answer;
    inconsistent["reference_count"] = json!(0);
    assert!(verify_media_scope(&document(inconsistent), 2).is_err());
}

#[test]
fn the_collision_gate_consumes_the_stored_result_under_the_project_policy() {
    let sheets = |rows: Value| json!({"project_settings": rows});
    let row = |count: u64| json!({"layers": {"collisions": count}});
    let default = collision_gate(&sheets(json!([])), Some(Some(&row(10))));
    assert!(default.allowed && default.checked && default.observed);
    let blocked = collision_gate(&sheets(json!([])), Some(Some(&row(11))));
    assert!(!blocked.allowed);
    assert_eq!(
        blocked.reason,
        Some("collision rows exceed the combined report threshold")
    );
    let strict =
        sheets(json!([{"parameter": "Allow_Combined_Reports_With_Collisions", "value": "no"}]));
    assert!(!collision_gate(&strict, Some(Some(&row(1)))).allowed);
    let tolerant = sheets(json!([{"parameter": COLLISION_MAX_SETTING, "value": "50"}]));
    assert!(collision_gate(&tolerant, Some(Some(&row(40)))).allowed);
    let zero = json!({"layers": {}, "report_metadata": {"outcomes": {"collision_detection": {"details": {"pairs": 0}}}}});
    assert!(collision_gate(&strict, Some(Some(&zero))).allowed);
    let never = collision_gate(&strict, Some(None));
    assert!(never.allowed && !never.checked && never.observed);
    let offline = collision_gate(&strict, None);
    assert!(offline.allowed && !offline.observed);
}

#[test]
fn voltage_drop_states_become_warnings_never_refusals() {
    let mut rows = BTreeMap::new();
    rows.insert("tx_missing".to_string(), json!({"name": "tx_missing"}));
    rows.insert(
        "tx_stale".to_string(),
        json!({"voltage_drop_metadata": {"state": "stale"}}),
    );
    let warnings = voltage_drop_warnings(
        &rows,
        &["tx_missing".into(), "tx_stale".into(), "tx_unread".into()],
    );
    assert_eq!(
        warnings
            .iter()
            .map(|w| (w.transformer.as_str(), w.state.as_str()))
            .collect::<Vec<_>>(),
        [("tx_missing", "missing"), ("tx_stale", "stale")]
    );
}

/// Contract 4: a combined reply is read by its family (task and prefix) and
/// the members this reader uses. A newer revision with added members is
/// admitted; a missing revision member or another family is refused by name.
#[test]
fn combined_replies_admit_same_family_revisions_and_refuse_other_families() {
    let refusal = |result: Result<VerifiedCombined, CombinedRefusal>| match result {
        Err(CombinedRefusal::Invalid { message }) => message,
        other => panic!("expected an invalid refusal, got {:?}", other.map(|_| ())),
    };
    let fixture = Fixture::new("Rwanda");
    let mut newer = fixture.result();
    newer["schema_version"] = json!(RESULT_SCHEMA_VERSION + 1);
    newer["added_member"] = json!({"from": "a newer Reporter"});
    newer["artifacts"][0]["added_member"] = json!(true);
    let verified = fixture.verify(&newer).unwrap();
    assert_eq!(verified.artifacts.len(), 3);
    let mut older = fixture.result();
    older["schema_version"] = json!(RESULT_SCHEMA_VERSION - 1);
    assert!(fixture.verify(&older).is_ok());

    let mut missing = fixture.result();
    missing.as_object_mut().unwrap().remove("schema_version");
    assert!(refusal(fixture.verify(&missing)).contains("schema_version"));
    let mut zero = fixture.result();
    zero["schema_version"] = json!(0);
    assert!(refusal(fixture.verify(&zero)).contains("schema_version"));
    let mut other_task = fixture.result();
    other_task["task"] = json!("export_transformer_report");
    assert!(refusal(fixture.verify(&other_task)).contains("family"));
    let mut other_prefix = fixture.result();
    other_prefix["prefix"] = json!("transformer");
    assert!(refusal(fixture.verify(&other_prefix)).contains("family"));
    let mut no_artifacts = fixture.result();
    no_artifacts.as_object_mut().unwrap().remove("artifacts");
    assert!(fixture.verify(&no_artifacts).is_err());

    let document = |value: Value| serde_json::to_vec(&value).unwrap();
    let media = json!({"schema_version": MEDIA_SCOPE_SCHEMA_VERSION + 1, "task": MEDIA_TASK, "status": "completed", "reference_count": 1, "source_projects": ["project-a"], "transformers_scanned": 2, "features_scanned": 9, "added_member": [1]});
    assert!(verify_media_scope(&document(media.clone()), 2).is_ok());
    let mut unversioned = media.clone();
    unversioned.as_object_mut().unwrap().remove("schema_version");
    assert!(verify_media_scope(&document(unversioned), 2)
        .unwrap_err()
        .contains("schema_version"));
    let mut foreign = media;
    foreign["task"] = json!(TASK);
    assert!(verify_media_scope(&document(foreign), 2)
        .unwrap_err()
        .contains("family"));
}
