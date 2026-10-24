use super::*;
use ds_project_data::revisions::PublishOutput;

fn head_json(engine: &str) -> Value {
    json!({
        "work_id": "work_abc",
        "engine": engine,
        "operation": "google_open_buildings",
        "variant": format!("p{}", "a".repeat(40)),
        "input_base_fingerprint": "b".repeat(64),
        "head_revision": 3,
        "updated_at": "2026-10-22T08:00:00Z",
        "outputs": [
            {"output_id": "manifest", "format": "json", "content_type": "application/json",
             "sha256": "c".repeat(64), "size_bytes": 900},
            {"output_id": "rev-000001", "format": "geojsonl_gz", "content_type": "application/gzip",
             "sha256": "d".repeat(64), "size_bytes": 12000}
        ]
    })
}

#[test]
fn a_publisher_crc32c_is_decoded_exactly() {
    assert_eq!(crc32c_of("AAAAAA=="), Some(0));
    // CRC32C("123456789") = 0xE3069283, as GCS would publish it.
    assert_eq!(crc32c_of("4waSgw=="), Some(0xE306_9283));
    assert_eq!(crc32c::crc32c(b"123456789"), 0xE306_9283);
    for refused in ["", "4waSgw", "4waSg===", "4wa$gw==", "4waSgwAA"] {
        assert_eq!(crc32c_of(refused), None, "{refused}");
    }
}

#[test]
fn a_head_answer_is_admitted_only_for_the_shared_dataset_engine() {
    let head = published(&head_json("project_dataset")).unwrap();
    assert_eq!(head.head_revision, 3);
    assert_eq!(head.outputs.len(), 2);
    assert_eq!(head.outputs[1].output_id, "rev-000001");
    assert!(published(&head_json("network_reporter")).is_err());
    let mut broken = head_json("project_dataset");
    broken["outputs"][1]["content_type"] = json!("application/json");
    assert!(published(&broken).is_err());
}

#[test]
fn a_revision_open_is_the_kernels_admitted_declaration() {
    let publication = Publication {
        head: Head {
            project: "gisagara".into(),
            operation: "google_open_buildings".into(),
            variant: format!("p{}", "a".repeat(40)),
        },
        base_fingerprint: chain::base_fingerprint(None),
        client_run_id: "seed-1-1-0".into(),
        outputs: vec![
            PublishOutput {
                output_id: chain::MANIFEST_OUTPUT.into(),
                format: chain::MANIFEST_FORMAT,
                content_type: chain::MANIFEST_CONTENT_TYPE,
                sha256: "c".repeat(64),
                size_bytes: 900,
                path: PathBuf::from("/nonexistent/manifest.json"),
            },
            PublishOutput {
                output_id: chain::output_id(1),
                format: chain::DELTA_FORMAT,
                content_type: chain::DELTA_CONTENT_TYPE,
                sha256: "d".repeat(64),
                size_bytes: 12_000,
                path: PathBuf::from("/nonexistent/rev.geojsonl.gz"),
            },
        ],
    };
    let engine = (
        format!("ds-network-reporter@0.1.0+{}", "e".repeat(40)),
        "f".repeat(64),
    );
    let body = open_body(&publication, &engine).unwrap();
    assert_eq!(body["engine"], "project_dataset");
    assert_eq!(
        body["input_base_fingerprint"],
        chain::base_fingerprint(None)
    );
    assert!(
        body["outputs"][0].get("path").is_none(),
        "local paths never travel"
    );
    let solar = ("ds-solar-engine@1.0.0".to_owned(), "f".repeat(64));
    assert!(open_body(&publication, &solar).is_err());
}

#[test]
fn finalize_answers_publish_or_a_typed_stale_verdict() {
    assert_eq!(
        finalized(&json!({"state": "published", "head_revision": 4})).unwrap(),
        Publish::Published { head_revision: 4 }
    );
    assert_eq!(
        finalized(&json!({"state": "stored_stale", "head_revision": 4,
            "conflict": "the current artifact head changed after this run opened"}))
        .unwrap(),
        Publish::Stale {
            reason: "the current artifact head changed after this run opened".into()
        }
    );
    assert!(finalized(&json!({"state": "pending"})).is_err());
    assert!(finalized(&json!({"state": "published"})).is_err());
}

#[test]
fn gateway_verdicts_map_to_the_declared_seed_refusals() {
    let refused = |status| {
        door_error(ds_sync_runtime::GatewayError::refused(
            status, None, "detail",
        ))
        .code()
        .to_owned()
    };
    assert_eq!(refused(403), "project_dataset_publication_refused");
    assert_eq!(refused(409), "project_dataset_revision_invalid");
    assert_eq!(
        door_error("offline".into()).code(),
        "dataset_provider_unavailable"
    );
    for code in [
        "project_dataset_publication_refused",
        "project_dataset_revision_invalid",
        "project_dataset_revision_stale",
        "reporter_engine_missing",
        "reporter_engine_incompatible",
        "contour_surface_uncovered",
        "engine_refused",
        "engine_response_invalid",
        "dataset_provider_unavailable",
    ] {
        assert!(
            SEED_COMMAND
                .refusals
                .iter()
                .any(|refusal| refusal.code == code),
            "seed declares {code}"
        );
    }
}

#[test]
fn the_publisher_object_path_is_encoded_and_fixed() {
    assert_eq!(
        encode_path("v3/polygons_s2_level_6_gzip_no_header/19c3_buildings.csv.gz"),
        "v3%2Fpolygons_s2_level_6_gzip_no_header%2F19c3_buildings.csv.gz"
    );
}

#[test]
fn contour_revisions_name_the_governed_surface() {
    assert!(surface_version().starts_with("rwanda_dem_10m/"));
}

fn seeded(uncovered: Vec<[f64; 4]>, answered: bool) -> ds_project_data::revisions::SharedOutcome {
    ds_project_data::revisions::SharedOutcome {
        ensure: ds_project_data::EnsureOutcome {
            dataset_key: "contours".into(),
            status: json!({}),
            coverage: Default::default(),
            coverage_geometry: None,
            clusters: 1,
            covered: uncovered.is_empty(),
            acquired: 1,
            skipped: 0,
            uncovered,
            features: if answered { 108 } else { 0 },
            warnings: vec![],
        },
        head_revision: answered.then_some(1),
        replayed: vec![],
        source_cells: 3,
        published: None,
        stale_retries: 0,
        chain: None,
        freshness: None,
        answered,
    }
}

#[test]
fn uncovered_contour_cells_are_an_itemised_gap_and_only_a_wholly_uncovered_project_refuses() {
    let goma = [29.20, -1.70, 29.21, -1.69];
    let border = [29.25, -1.70, 29.26, -1.69];
    // Mixed: the covered cells published, the uncovered one is itemised.
    let gaps = surface_gaps(&seeded(vec![goma], true), &[border])
        .unwrap()
        .expect("a gap is listed");
    assert_eq!(gaps[0]["reason"], OUTSIDE_DEM);
    assert_eq!(gaps[0]["count"], 1);
    assert_eq!(gaps[0]["cells"], json!([goma]));
    assert_eq!(gaps[1]["reason"], "partially_outside_rwanda_dem");
    assert_eq!(gaps[1]["published"], true);
    // Fully covered: no gap, the row is unchanged.
    assert_eq!(surface_gaps(&seeded(vec![], true), &[]).unwrap(), None);
    // Control: the surface reaches none of the project — the typed refusal.
    let (code, message) = surface_gaps(&seeded(vec![goma, border], false), &[]).unwrap_err();
    assert_eq!(code, "contour_surface_uncovered");
    assert!(message.contains("covers none of this project's 2 planned contour cell(s)"));
}
