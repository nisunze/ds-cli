use super::*;
use crate::project_dataset_cache::{Dataset, Quality};

fn buildings(provider: &str) -> Dataset {
    Dataset {
        id: "google_open_buildings".into(),
        provider: provider.into(),
        quality: Quality::Source,
        parameters: BTreeMap::from([("context_kind".into(), json!("google_open_buildings"))]),
        source_version: String::new(),
        crs: "EPSG:4326".into(),
    }
}

fn cells(cells: &[[f64; 4]]) -> Coverage {
    Coverage {
        cells: cells.to_vec(),
    }
}

fn draft(coverage: Coverage, version: &str, refresh: bool) -> Draft {
    Draft {
        coverage,
        source_version: version.into(),
        source_receipt: json!({"publisher": "google", "tiles": ["19c3"]}),
        feature_count: 3,
        sha256: "a".repeat(64),
        size_bytes: 120,
        refresh,
    }
}

#[test]
fn rwanda_tiles_are_the_publishers_own_level_six_names() {
    // Independently computed and confirmed against the publisher's bucket
    // listing (`v3/polygons_s2_level_6_gzip_no_header/<token>_buildings.csv.gz`).
    assert_eq!(s2_token(-1.95, 30.06, 6), "19dd", "Kigali");
    assert_eq!(s2_token(-2.6, 29.74, 6), "19c3", "Huye");
    assert_eq!(s2_token(0.0, 0.0, 0), "1");
    let rwanda = google_open_buildings::tiles(&cells(&[[28.86, -2.84, 30.9, -1.05]])).unwrap();
    assert_eq!(rwanda, ["19c3", "19c5", "19d9", "19db", "19dd", "19df"]);
    let gisagara = google_open_buildings::tiles(&cells(&[[29.84, -2.62, 29.86, -2.6]])).unwrap();
    assert_eq!(gisagara, ["19c3"]);
    assert_eq!(
        google_open_buildings::object("19c3").unwrap(),
        "v3/polygons_s2_level_6_gzip_no_header/19c3_buildings.csv.gz"
    );
    for refused in ["19C3", "19c", "../x", "19c3/"] {
        assert!(google_open_buildings::object(refused).is_err(), "{refused}");
    }
}

#[test]
fn a_tile_search_wider_than_a_project_refuses() {
    assert!(google_open_buildings::tiles(&cells(&[[0.0, -10.0, 40.0, 10.0]])).is_err());
}

#[test]
fn the_shared_head_ignores_the_room_provider_but_not_parameters() {
    let a = buildings("ds-brain:query_print_context");
    let b = buildings("any-local-room-token");
    assert_eq!(head_variant(&a), head_variant(&b));
    assert_eq!(head_operation(&a).unwrap(), "google_open_buildings");
    let mut contours = a.clone();
    contours
        .parameters
        .insert("minor_interval_m".into(), json!(5));
    assert_ne!(head_variant(&a), head_variant(&contours));
    assert!(artifact_id(&head_variant(&a)));
}

#[test]
fn the_genesis_base_is_the_digest_of_no_manifest() {
    assert_eq!(
        base_fingerprint(None),
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
    assert_ne!(base_fingerprint(Some(b"{}")), base_fingerprint(None));
}

#[test]
fn a_chain_round_trips_and_admits_additive_fields() {
    let dataset = buildings("ds-brain:query_print_context");
    let first = next(
        None,
        "project-a",
        &dataset,
        draft(cells(&[[30.0, -2.0, 30.1, -1.9]]), "v3", false),
    )
    .unwrap();
    let second = next(
        Some(&first),
        "project-a",
        &dataset,
        draft(cells(&[[30.2, -2.0, 30.3, -1.9]]), "v3", false),
    )
    .unwrap();
    assert_eq!(second.revisions[1].output_id, "rev-000002");
    let bytes = encode(&second).unwrap();
    assert_eq!(decode(&bytes, "project-a", &dataset).unwrap(), second);
    let mut additive: Value = serde_json::from_slice(&bytes).unwrap();
    additive["published_note"] = json!("a later publisher field");
    additive["revisions"][0]["compacted_from"] = json!([]);
    let admitted = decode(
        &serde_json::to_vec(&additive).unwrap(),
        "project-a",
        &dataset,
    )
    .unwrap();
    assert_eq!(admitted, second);
    assert_eq!(
        coverage(&second).unwrap().cells,
        vec![[30.0, -2.0, 30.1, -1.9], [30.2, -2.0, 30.3, -1.9]]
    );
}

#[test]
fn a_foreign_out_of_order_or_unbounded_chain_refuses_whole() {
    let dataset = buildings("ds-brain:query_print_context");
    let chain = next(
        None,
        "project-a",
        &dataset,
        draft(cells(&[[30.0, -2.0, 30.1, -1.9]]), "v3", false),
    )
    .unwrap();
    let bytes = encode(&chain).unwrap();
    assert!(decode(&bytes, "project-b", &dataset).is_err());
    let mut other = dataset.clone();
    other
        .parameters
        .insert("minor_interval_m".into(), json!(10));
    assert!(decode(&bytes, "project-a", &other).is_err());
    let mutations: [fn(&mut Value); 8] = [
        |v: &mut Value| v["revisions"][0]["revision"] = json!(2),
        |v: &mut Value| v["revisions"][0]["output_id"] = json!("rev-1"),
        |v: &mut Value| v["revisions"][0]["sha256"] = json!("A".repeat(64)),
        |v: &mut Value| v["revisions"][0]["size_bytes"] = json!(0),
        |v: &mut Value| v["revisions"][0]["coverage"] = json!({"cells": []}),
        |v: &mut Value| v["revisions"][0]["source_version"] = json!(""),
        |v: &mut Value| v["schema"] = json!("ds.project-dataset-revisions/v0"),
        |v: &mut Value| v["revisions"] = json!([]),
    ];
    for mutate in mutations {
        let mut value: Value = serde_json::from_slice(&bytes).unwrap();
        mutate(&mut value);
        assert!(
            decode(&serde_json::to_vec(&value).unwrap(), "project-a", &dataset).is_err(),
            "{value}"
        );
    }
    let mut full = chain.clone();
    while full.revisions.len() < MAX_REVISIONS {
        full = next(
            Some(&full),
            "project-a",
            &dataset,
            draft(cells(&[[30.0, -2.0, 30.1, -1.9]]), "v3", true),
        )
        .unwrap();
    }
    assert!(next(
        Some(&full),
        "project-a",
        &dataset,
        draft(cells(&[[30.0, -2.0, 30.1, -1.9]]), "v3", true)
    )
    .is_err());
}

#[test]
fn a_seed_publishes_only_completed_coverage_the_chain_lacks() {
    let wanted = cells(&[[30.0, -2.0, 30.4, -1.9]]);
    let held = cells(&[[30.0, -2.0, 30.4, -1.9]]);
    let published = cells(&[[30.0, -2.0, 30.2, -1.9]]);
    assert_eq!(
        delta(&wanted, &held, &published, false).unwrap().cells,
        vec![[30.2, -2.0, 30.4, -1.9]]
    );
    // Nothing completed here is never published, refresh or not.
    let partial = cells(&[[30.0, -2.0, 30.1, -1.9]]);
    assert_eq!(
        delta(&wanted, &partial, &Coverage::default(), false)
            .unwrap()
            .cells,
        partial.cells
    );
    assert!(delta(&wanted, &held, &wanted, false).unwrap().is_empty());
    // An explicit refresh republishes what it completed, held or not.
    assert_eq!(
        delta(&wanted, &held, &wanted, true).unwrap().cells,
        wanted.cells
    );
}

#[test]
fn readers_apply_only_new_revisions_in_order() {
    let dataset = buildings("ds-brain:query_print_context");
    let mut chain = next(
        None,
        "p",
        &dataset,
        draft(cells(&[[30.0, -2.0, 30.1, -1.9]]), "v3", false),
    )
    .unwrap();
    for _ in 0..2 {
        chain = next(
            Some(&chain),
            "p",
            &dataset,
            draft(cells(&[[30.0, -2.0, 30.1, -1.9]]), "v3", false),
        )
        .unwrap();
    }
    let applied = BTreeSet::from([1, 2]);
    let todo: Vec<u32> = pending(&chain, &applied)
        .iter()
        .map(|r| r.revision)
        .collect();
    assert_eq!(todo, vec![3]);
    assert_eq!(pending(&chain, &BTreeSet::new()).len(), 3);
}

#[test]
fn a_publisher_version_change_is_reported_until_a_refresh_supersedes_it() {
    let dataset = buildings("ds-brain:query_print_context");
    let a = [30.0, -2.0, 30.1, -1.9];
    let b = [30.2, -2.0, 30.3, -1.9];
    let chain = next(None, "p", &dataset, draft(cells(&[a]), "v3", false)).unwrap();
    let chain = next(Some(&chain), "p", &dataset, draft(cells(&[b]), "v4", false)).unwrap();
    let report = chain_freshness(&chain, "v4").unwrap();
    assert!(!report.fresh);
    assert_eq!(report.stale.cells, vec![a]);
    // Nothing is stale while the current version is unknown or unchanged.
    assert!(chain_freshness(&chain, "").unwrap().fresh);
    let refreshed = next(Some(&chain), "p", &dataset, draft(cells(&[a]), "v4", true)).unwrap();
    assert!(chain_freshness(&refreshed, "v4").unwrap().fresh);
    assert_eq!(summary(&refreshed)["revisions"], 3);
}

#[test]
fn the_native_publication_door_admits_exactly_one_revision_shape() {
    let dataset = buildings("ds-brain:query_print_context");
    let open = |outputs: Value| {
        json!({"action":"open","project_id":"project-a","client_run_id":"seed-1",
            "engine":ENGINE,"operation":head_operation(&dataset).unwrap(),"variant":head_variant(&dataset),
            "engine_version":format!("ds-network-reporter@0.1.0+{}", "d".repeat(40)),
            "engine_build_manifest_sha256":"e".repeat(64),
            "input_base_fingerprint":base_fingerprint(None),
            "outputs":outputs})
    };
    let manifest = json!({"output_id":"manifest","format":"json","content_type":"application/json","sha256":"a".repeat(64),"size_bytes":10});
    let delta = json!({"output_id":"rev-000001","format":"geojsonl_gz","content_type":"application/gzip","sha256":"b".repeat(64),"size_bytes":20});
    let admitted = open(json!([manifest, delta]));
    assert_eq!(
        crate::sync::declaration::validate("project-a", &admitted).unwrap(),
        crate::sync::declaration::Action::Open
    );
    for refused in [
        open(json!([manifest])),
        open(json!([
            delta,
            json!({"output_id":"rev-000002","format":"geojsonl_gz","content_type":"application/gzip","sha256":"b".repeat(64),"size_bytes":20})
        ])),
        open(json!([
            manifest,
            json!({"output_id":"rev-000001","format":"json","content_type":"application/json","sha256":"b".repeat(64),"size_bytes":20})
        ])),
        open(json!([
            manifest,
            json!({"output_id":"report","format":"geojsonl_gz","content_type":"application/gzip","sha256":"b".repeat(64),"size_bytes":20})
        ])),
        open(json!([
            manifest,
            json!({"output_id":"rev-000001","format":"geojsonl_gz","content_type":"application/gzip","sha256":"b".repeat(64),"size_bytes": MAX_DELTA_BYTES + 1})
        ])),
    ] {
        assert!(
            crate::sync::declaration::validate("project-a", &refused).is_err(),
            "{refused}"
        );
    }
    let mut foreign_variant = admitted.clone();
    foreign_variant["variant"] = json!("default");
    assert!(crate::sync::declaration::validate("project-a", &foreign_variant).is_err());
    let mut other_engine = admitted.clone();
    other_engine["engine_version"] = json!("ds-solar-engine@1.0.0");
    assert!(crate::sync::declaration::validate("project-a", &other_engine).is_err());
    // The outbox registry never learns this engine: revisions are not reports.
    assert!(crate::sync::engines::engine(ENGINE).is_none());
}
