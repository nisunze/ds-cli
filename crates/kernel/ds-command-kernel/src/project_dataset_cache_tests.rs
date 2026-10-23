use super::*;

#[test]
fn sheet_capture_edges_are_outward_quantized_for_room_queries() {
    let rect = Rect::from_bounds([29.905891855, -2.554536198, 29.908155482, -2.550060858]).unwrap();
    let captured = capture(rect, 200.0, &both_orientations());
    for edge in captured.bounds() {
        assert_eq!(round(edge), edge);
    }
    let geometry = Coverage::from_rects(vec![captured]).geometry().unwrap();
    assert!(crate::reference_cache::covers_area(&geometry, &geometry).unwrap());
}

fn catalog() -> CatalogEntry {
    CatalogEntry {
        id: "buildings".into(),
        label: "Buildings".into(),
        provider: "ds".into(),
        quality: "source".into(),
        parameters: BTreeMap::new(),
        residency: String::new(),
        unpublished: false,
    }
}

#[test]
fn retired_held_layers_do_not_return_as_active_datasets() {
    let policy = buffer_policy(&[]).unwrap();
    let held = json!({
        "exists": true, "ready": true, "index_state": "ready",
        "feature_count": 42, "fresh": true,
    });
    let row = |id: &str, layer: &str| OverviewRow {
        dataset: CatalogEntry {
            id: id.into(),
            parameters: BTreeMap::from([("layer".into(), json!(layer))]),
            ..catalog()
        },
        held: held.clone(),
    };
    let mut other_country = row("other-country-power-digest", "powerlines");
    other_country
        .dataset
        .parameters
        .insert("country".into(), json!("Burkina Faso"));
    let rows = vec![
        row("retired-schools-digest", "elementary_school"),
        row("retired-power-digest", "powerlines"),
        row("primary-schools-digest", "primary_schools"),
        row("hv-digest", "hv_line"),
        row("unlisted-retained-digest", "operator_layer"),
        other_country,
    ];
    let before: Vec<_> = rows.iter().map(|row| row.held.clone()).collect();
    let answer = overview("p1", &policy, &rows);
    let ids: Vec<_> = answer["datasets"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| row["dataset_id"].as_str().unwrap())
        .collect();
    assert_eq!(
        ids,
        [
            "primary-schools-digest",
            "hv-digest",
            "unlisted-retained-digest",
            "other-country-power-digest"
        ]
    );
    assert_eq!(answer["available"], 4);
    assert_eq!(answer["seeded"], 4);
    assert!(answer["datasets"]
        .as_array()
        .unwrap()
        .iter()
        .all(|row| row["ready"] == true && row["feature_count"] == 42));
    assert_eq!(
        rows.iter().map(|row| row.held.clone()).collect::<Vec<_>>(),
        before
    );
    // Old named rooms without parameters still obey the same retirement.
    assert_eq!(
        retired_dataset_layer(&CatalogEntry {
            id: "powerlines".into(),
            ..catalog()
        }),
        Some("powerlines")
    );
}

/// The projection the card and the command line both read. Absent is not
/// empty, a partial room keeps its own defaults, and no dataset speaks for
/// another.
#[test]
fn an_overview_reports_absence_apart_from_emptiness() {
    let policy = buffer_policy(&[50.0]).unwrap();
    let never = OverviewRow {
        dataset: catalog(),
        held: Value::Null,
    };
    let requested_then_failed = OverviewRow {
        dataset: CatalogEntry {
            id: "roads".into(),
            ..catalog()
        },
        held: json!({
            "exists": true, "ready": false, "index_state": "absent",
            "last_error": "provider refused", "pending_queries": 0,
            "requested": {"cells": ["a"]}, "completed": {"cells": []},
            "feature_count": 0, "fresh": true
        }),
    };
    let covered = OverviewRow {
        dataset: CatalogEntry {
            id: "parcels".into(),
            ..catalog()
        },
        held: json!({
            "exists": true, "ready": true, "index_state": "ready",
            "feature_count": 12, "source_version": "v2",
            "available_version": "v3", "fresh": false,
            "stale": {"cells": ["a"]}, "stale_count": 1,
            "requested": {"cells": ["a"]}, "completed": {"cells": ["a"]},
            "queries": (0..25).map(|index| json!({"id": index})).collect::<Vec<_>>()
        }),
    };
    let answer = overview("p1", &policy, &[never, requested_then_failed, covered]);
    assert_eq!(answer["project"], "p1");
    assert_eq!(answer["available"], 3);
    assert_eq!(answer["seeded"], 2);
    let rows = answer["datasets"].as_array().unwrap();
    // Never seeded: absent, not an empty room, and it inherits the policy.
    assert_eq!(rows[0]["seeded"], false);
    assert_eq!(rows[0]["index_state"], "absent");
    assert_eq!(rows[0]["fresh"], true);
    assert_eq!(rows[0]["completed"], json!({"cells": []}));
    assert_eq!(
        rows[0]["buffer_policy"],
        serde_json::to_value(&policy).unwrap()
    );
    assert_eq!(rows[0]["created_at"], Value::Null);
    // Local, and saying so on every row — held or not. A room is one
    // machine's holding; nothing in this cache publishes a dataset, so an
    // operator comparing two installations reads a fact rather than
    // inferring one from a row that merely says "held".
    assert_eq!(rows[0]["local_holding"], false);
    assert_eq!(rows[0]["shared"], false);
    // Requested but failed: seeded and unready, with its own error.
    assert_eq!(rows[1]["seeded"], true);
    assert_eq!(rows[1]["local_holding"], true);
    assert_eq!(rows[1]["shared"], false);
    assert_eq!(rows[1]["ready"], false);
    assert_eq!(rows[1]["last_error"], "provider refused");
    // Superseded: every feature kept, staleness reported beside the row.
    assert_eq!(rows[2]["fresh"], false);
    assert_eq!(rows[2]["stale_count"], 1);
    assert_eq!(rows[2]["feature_count"], 12);
    // The query log is bounded so a long history cannot become the answer.
    assert_eq!(rows[2]["queries"].as_array().unwrap().len(), 20);
}

/// `ready` is one decision with a named reason
/// (`docs/contracts/foundation-datasets.md` R5): the first failing
/// condition, in a fixed order, and none when ready.
#[test]
fn readiness_names_the_first_failing_condition() {
    let empty = Coverage::default();
    let one = Coverage::from_rects(vec![Rect::from_bounds([30.0, -2.0, 30.01, -1.99]).unwrap()]);
    let two = Coverage::from_rects(vec![
        Rect::from_bounds([30.0, -2.0, 30.01, -1.99]).unwrap(),
        Rect::from_bounds([30.02, -2.0, 30.03, -1.99]).unwrap(),
    ]);
    let reason = |index: &str,
                  pending: i64,
                  completed: &Coverage,
                  requested: &Coverage,
                  error: Option<&str>| {
        readiness(index, pending, completed, requested, error).unwrap()
    };
    assert_eq!(
        reason("absent", 0, &one, &one, None)
            .ready_reason
            .as_deref(),
        Some("index_absent")
    );
    assert_eq!(
        reason("incomplete", 0, &one, &one, None)
            .ready_reason
            .as_deref(),
        Some("index_incomplete")
    );
    assert_eq!(
        reason("ready", 2, &one, &one, None).ready_reason.as_deref(),
        Some("acquisition_pending (2)")
    );
    assert_eq!(
        reason("ready", 0, &empty, &one, None)
            .ready_reason
            .as_deref(),
        Some("nothing_completed")
    );
    assert_eq!(
        reason("ready", 0, &empty, &one, Some("provider refused"))
            .ready_reason
            .as_deref(),
        Some("acquisition_failed: provider refused")
    );
    assert_eq!(
        reason("ready", 0, &one, &two, None).ready_reason.as_deref(),
        Some("coverage_gap (1 cells)")
    );
    // An empty room whose index answers is ready: asking again would spend
    // money to learn the same emptiness.
    let ready = reason("empty", 0, &one, &one, Some("an old error"));
    assert!(ready.ready && ready.ready_reason.is_none());
    let ready = reason("ready", 0, &two, &one, None);
    assert!(ready.ready && ready.ready_reason.is_none());
}

/// A cloud-resident row is never held here and the overview says so; a
/// bundle row nobody seeded says why there is no room to be ready.
#[test]
fn an_overview_carries_residency_and_the_reason_a_row_is_not_ready() {
    let policy = buffer_policy(&[50.0]).unwrap();
    let cloud = OverviewRow {
        dataset: CatalogEntry {
            id: "parcels".into(),
            residency: "cloud".into(),
            ..catalog()
        },
        held: Value::Null,
    };
    let unpublished = OverviewRow {
        dataset: CatalogEntry {
            id: "wetlands".into(),
            unpublished: true,
            ..catalog()
        },
        held: Value::Null,
    };
    let held = OverviewRow {
        dataset: CatalogEntry {
            id: "roads".into(),
            ..catalog()
        },
        held: json!({
            "exists": true, "ready": false, "ready_reason": "coverage_gap (3 cells)",
            "index_state": "ready", "feature_count": 9, "pending_queries": 0,
            "requested": {"cells": ["a"]}, "completed": {"cells": ["a"]}, "fresh": true
        }),
    };
    // The same cloud row once its cells were seeded through the bounded
    // read: a room like any other, ready on its own coverage.
    let seeded_cloud = OverviewRow {
        dataset: cloud.dataset.clone(),
        held: json!({
            "exists": true, "ready": true, "ready_reason": Value::Null,
            "index_state": "ready", "feature_count": 10358, "pending_queries": 0,
            "requested": {"cells": ["a"]}, "completed": {"cells": ["a"]}, "fresh": true
        }),
    };
    let answer = overview(
        "p1",
        &policy,
        &[
            cloud,
            unpublished,
            held,
            OverviewRow {
                dataset: catalog(),
                held: Value::Null,
            },
            seeded_cloud,
        ],
    );
    let rows = answer["datasets"].as_array().unwrap();
    assert_eq!(rows[0]["residency"], "cloud");
    assert_eq!(rows[0]["ready"], false);
    assert_eq!(rows[0]["ready_reason"], READY_REASON_CLOUD_RESIDENT);
    assert_eq!(rows[4]["residency"], "cloud");
    assert_eq!(rows[4]["seeded"], true);
    assert_eq!(rows[4]["ready"], true);
    assert_eq!(rows[4]["ready_reason"], Value::Null);
    assert_eq!(rows[4]["feature_count"], 10358);
    assert_eq!(rows[1]["residency"], "bundle");
    assert_eq!(rows[1]["ready_reason"], READY_REASON_BUNDLE_UNPUBLISHED);
    assert_eq!(rows[2]["ready_reason"], "coverage_gap (3 cells)");
    assert_eq!(rows[3]["ready_reason"], READY_REASON_NOT_SEEDED);
    // The rectangle bound a cloud read is measured against.
    assert!(bounds_area_m2([29.57, -2.53, 29.59, -2.51]).unwrap() < MAX_ENVELOPE_M2);
    assert!(bounds_area_m2([29.5, -2.6, 29.9, -2.3]).unwrap() > MAX_ENVELOPE_M2);
    assert!(bounds_area_m2([29.6, -2.5, 29.5, -2.4]).is_err());
}

/// A room that says it does not exist reports as absent even when stale
/// members survive beside the flag — `exists` is the one gate.
#[test]
fn a_tombstoned_room_reports_as_absent() {
    let policy = buffer_policy(&[50.0]).unwrap();
    let answer = overview(
        "p1",
        &policy,
        &[OverviewRow {
            dataset: catalog(),
            held: json!({"exists": false, "feature_count": 9, "index_state": "ready"}),
        }],
    );
    assert_eq!(answer["seeded"], 0);
    assert_eq!(answer["datasets"][0]["feature_count"], 0);
    assert_eq!(answer["datasets"][0]["index_state"], "absent");
}

#[test]
fn a_district_sized_chain_queries_member_rectangles_and_focus_narrows_them() {
    // Forty designed sites chained 350 m apart along two 7 km legs: every
    // 200 m buffer overlaps its neighbour, so they fuse into one cluster
    // whose envelope (~7 km x 7 km) is far past MAX_ENVELOPE_M2.
    let step = 0.00315; // ~350 m
    let mut extents = Vec::new();
    for i in 0..20 {
        let x = 30.0 + step * i as f64;
        extents.push(extent(
            &format!("x{i}"),
            Some([x, -2.0, x + 0.0001, -1.9999]),
        ));
        let y = -2.0 + step * i as f64;
        extents.push(extent(
            &format!("y{i}"),
            Some([30.0, y, 30.0001, y + 0.0001]),
        ));
    }
    let whole = plan(&scope(), &buildings(), &policy(), &extents).unwrap();
    assert_eq!(whole.clusters.len(), 1);
    assert!(whole.clusters[0].split);
    assert_eq!(whole.clusters[0].members.len(), 40);
    assert_eq!(whole.coverage.cells.len(), 40);
    for &[w, s, e, n] in &whole.coverage.cells {
        assert!(rect_area_m2(Rect { w, s, e, n }) < 1_000_000.0);
    }
    let one = plan_focused(
        &scope(),
        &buildings(),
        &policy(),
        &extents,
        &["x7".to_string()],
    )
    .unwrap();
    assert_eq!(one.coverage.cells.len(), 1);
    assert_eq!(one.clusters[0].cells, 1);
    assert_eq!(one.clusters[0].members.len(), 40);
    let none = plan_focused(
        &scope(),
        &buildings(),
        &policy(),
        &extents,
        &["absent".to_string()],
    )
    .unwrap();
    assert!(none.empty);
    // A small pair keeps its single envelope and is not split.
    let pair = [
        extent("a", Some([30.5, -2.5, 30.501, -2.499])),
        extent("b", Some([30.5012, -2.5, 30.5022, -2.499])),
    ];
    let small = plan(&scope(), &buildings(), &policy(), &pair).unwrap();
    assert_eq!(small.coverage.cells.len(), 1);
    assert!(!small.clusters[0].split);
}

#[test]
fn shared_buffers_never_shrink_below_project_acquisition_defaults() {
    assert_eq!(buffer_policy(&[100.]).unwrap().design_buffer_m, 200.);
    assert_eq!(buffer_policy(&[100.]).unwrap().isolated_buffer_m, 1000.);
    assert_eq!(buffer_policy(&[500.]).unwrap().design_buffer_m, 500.);
    assert_eq!(buffer_policy(&[1500.]).unwrap().isolated_buffer_m, 1500.);
    assert!(buffer_policy(&[f64::NAN]).is_err());
    assert!(!buffer_policy(&[]).unwrap().provisional);
}

#[test]
fn cloud_read_decoder_seeds_only_a_complete_cell() {
    let area = json!({"type":"Polygon","coordinates":[[[29.,-3.],[31.,-3.],[31.,-1.],[29.,-1.],[29.,-3.]]]});
    let feature = json!({"type":"Feature","geometry":{"type":"Point","coordinates":[30.,-2.]},
            "properties":{"customer_segmentation":"Residential"}});
    let valid = json!({"status":"success",
            "dataset":{"id":"abc","layer":"edcl_customers","residency":"cloud","version":"sha-1","source":"p.d.t"},
            "query":{"bound":{"kind":"boundary"}},
            "features":[feature.clone()],"rows_cap":5000,"rows_returned":1,"rows_total":1,"truncated":false});
    let page = decode_cloud_read(&valid, &area, "edcl_customers").unwrap();
    assert_eq!(page["sourceVersion"], "sha-1");
    assert_eq!(page["features"].as_array().unwrap().len(), 1);
    assert_eq!(page["sourceReceipt"]["dataset"]["layer"], "edcl_customers");
    assert_eq!(page["sourceReceipt"]["rows_total"], 1);
    // Another layer's answer never seeds this dataset.
    assert!(decode_cloud_read(&valid, &area, "rwanda_upi_parcels").is_err());
    // A truncated cell can never extend coverage: the room would lie.
    let mut truncated = valid.clone();
    truncated["rows_total"] = json!(9000);
    truncated["truncated"] = json!(true);
    let refusal = decode_cloud_read(&truncated, &area, "edcl_customers").unwrap_err();
    assert!(refusal.contains("truncated"), "{refusal}");
    // Counts that disagree with the features are a malformed answer.
    let mut miscounted = valid.clone();
    miscounted["rows_returned"] = json!(2);
    assert!(decode_cloud_read(&miscounted, &area, "edcl_customers").is_err());
    let mut bundle = valid.clone();
    bundle["dataset"]["residency"] = json!("bundle");
    assert!(decode_cloud_read(&bundle, &area, "edcl_customers").is_err());
    let mut malformed = valid.clone();
    malformed["features"] = json!([{"type":"Feature","geometry":null}]);
    assert!(decode_cloud_read(&malformed, &area, "edcl_customers").is_err());
    // The inverted world ring a BigQuery export writes never reaches a room.
    let mut inverted = valid.clone();
    inverted["features"] = json!([{"type":"Feature","properties":{},"geometry":{"type":"Polygon","coordinates":[
            [[180.,90.],[-180.,90.],[-180.,-90.],[180.,-90.],[180.,90.]],
            [[30.,-2.],[30.1,-2.],[30.1,-2.1],[30.,-2.1],[30.,-2.]]]}}]);
    let refusal = decode_cloud_read(&inverted, &area, "edcl_customers").unwrap_err();
    assert!(
        refusal.starts_with("cloud read feature 0 is not admitted: polygon_covers_hemisphere: "),
        "{refusal}"
    );
}

#[test]
fn contour_decoder_synthesises_a_bounded_receipt_and_keeps_both_kinds() {
    let area = json!({"type":"Polygon","coordinates":[[[30.,-1.],[30.01,-1.],[30.01,-0.99],[30.,-0.99],[30.,-1.]]]});
    let line = |kind: &str, lat: f64| {
        json!({"type":"Feature","properties":{"elevation_m":1500,"contour_kind":kind,
                "contour_label":"","interval_m":10,"source":"rwanda_dem_10m"},
                "geometry":{"type":"LineString","coordinates":[[30.002,lat],[30.004,lat]]}})
    };
    let valid = json!({"status":"success",
            "layers":{
                "contours_intermediate":{"type":"FeatureCollection","features":[line("intermediate",-0.995),line("intermediate",-0.994)]},
                "contours_index":{"type":"FeatureCollection","features":[line("index",-0.993)]}},
            "source":"rwanda_dem_10m","source_version":"https://dem.example/rwanda/10m",
            "minor_interval_m":10,"index_interval_m":50,"requested_spacing_m":10.0,
            "effective_spacing_m":10.0,"grid":[112,111],"sampled_points":12432,
            "unresolved_points":0,"intermediate_features":2,"index_features":1});
    let page = decode_contours(&valid, &area).unwrap();
    assert_eq!(page["features"].as_array().unwrap().len(), 3);
    assert_eq!(page["sourceVersion"], "https://dem.example/rwanda/10m");
    let receipt = &page["sourceReceipt"];
    assert!(validate_source_receipt(receipt).is_ok());
    assert_eq!(receipt["source"], "rwanda_dem_10m");
    assert_eq!(receipt["grid"], json!([112, 111]));
    assert_eq!(receipt["minor_interval_m"], 10);
    assert_eq!(receipt["index_interval_m"], 50);
    assert_eq!(receipt["unresolved_points"], 0);
    // The kinds survive for the print-context split.
    let kinds: Vec<&str> = page["features"]
        .as_array()
        .unwrap()
        .iter()
        .map(|feature| feature["properties"]["contour_kind"].as_str().unwrap())
        .collect();
    assert_eq!(kinds, ["intermediate", "intermediate", "index"]);
    // A version past its bound is refused, not trimmed.
    let mut long = valid.clone();
    long["source_version"] = json!("v".repeat(513));
    assert_eq!(
        decode_contours(&long, &area).unwrap_err(),
        "contour source version exceeds its bound"
    );
    // Wholly unresolved sampling has no surface to cut from.
    let mut blind = valid.clone();
    blind["unresolved_points"] = json!(12432);
    assert_eq!(
        decode_contours(&blind, &area).unwrap_err(),
        "contour acquisition resolved no elevation sample"
    );
    // The door reaches the same decoder.
    let request = json!({"action":"decode_contours","schema":SCHEMA,"response":valid,"area":area});
    let answer: Value =
        serde_json::from_str(&evaluate(&serde_json::to_vec(&request).unwrap()).unwrap()).unwrap();
    assert_eq!(answer["sourceReceipt"]["source"], "rwanda_dem_10m");
}

fn scope() -> Scope {
    Scope {
        principal: "uid-1".into(),
        project: "project-a".into(),
    }
}

fn buildings() -> Dataset {
    Dataset {
        id: "google_open_buildings".into(),
        provider: "ds-brain:query_print_context".into(),
        quality: Quality::Source,
        parameters: BTreeMap::from([
            ("min_area_m2".into(), json!(25)),
            ("min_confidence".into(), json!(0.7)),
        ]),
        source_version: String::new(),
        crs: default_crs(),
    }
}

fn policy() -> BufferPolicy {
    BufferPolicy {
        design_buffer_m: 200.0,
        isolated_buffer_m: 1_000.0,
        isolated_rule: IsolatedRule::PointOnly,
        provisional: true,
        sheets: both_orientations(),
        area: AcquisitionArea::ClusterEnvelope,
    }
}

fn extent(name: &str, bounds: Option<[f64; 4]>) -> Extent {
    Extent {
        name: name.into(),
        bounds,
    }
}

// An independent WGS84 yardstick. A spherical haversine would be the wrong
// ruler here — it disagrees with the ellipsoid by ~0.1% and would read a
// correct 500 m buffer as 499.4 m, so the test would be measuring its own
// approximation rather than the code.
const SEMI_MAJOR: f64 = 6_378_137.0;
const ECCENTRICITY_SQUARED: f64 = 0.006_694_379_990_141_316;

/// Metres north between two latitudes on the same meridian.
fn northing(from: f64, to: f64) -> f64 {
    let phi = ((from + to) / 2.0).to_radians();
    let meridian = SEMI_MAJOR * (1.0 - ECCENTRICITY_SQUARED)
        / (1.0 - ECCENTRICITY_SQUARED * phi.sin().powi(2)).powf(1.5);
    (to - from).to_radians().abs() * meridian
}

/// Metres east between two longitudes along one parallel.
fn easting(latitude: f64, from: f64, to: f64) -> f64 {
    let phi = latitude.to_radians();
    let prime_vertical = SEMI_MAJOR / (1.0 - ECCENTRICITY_SQUARED * phi.sin().powi(2)).sqrt();
    (to - from).to_radians().abs() * prime_vertical * phi.cos()
}

#[test]
fn buffer_is_metric_at_every_latitude_and_never_short() {
    for latitude in [0.0, -1.94, 45.0, -60.0] {
        let rect = Rect::from_bounds([30.0, latitude, 30.0, latitude]).unwrap();
        let grown = buffered(rect, 500.0);
        for distance in [
            northing(latitude, grown.n),
            northing(grown.s, latitude),
            easting(latitude, 30.0, grown.e),
            easting(latitude, grown.w, 30.0),
        ] {
            assert!(
                (500.0..502.0).contains(&distance),
                "latitude {latitude} produced {distance} m"
            );
        }
    }
}

#[test]
fn disconnected_designs_stay_separate_and_neighbours_fuse() {
    // Two transformers 200 m apart fuse under a 500 m buffer; a third sits
    // a whole degree away and must never join their rectangle.
    let plan = plan(
        &scope(),
        &buildings(),
        &policy(),
        &[
            extent("T1", Some([30.0, -2.0, 30.002, -1.998])),
            extent("T2", Some([30.0025, -2.0, 30.004, -1.998])),
            extent("T3", Some([31.0, -2.0, 31.002, -1.998])),
        ],
    )
    .unwrap();
    assert_eq!(plan.clusters.len(), 2);
    assert_eq!(plan.clusters[0].members, vec!["T1", "T2"]);
    assert_eq!(plan.clusters[1].members, vec!["T3"]);
    // The fused pair is one rectangle, not two, and the far transformer is
    // a separate one — never a single envelope spanning the degree between.
    assert_eq!(plan.coverage.cells.len(), 2);
    let spans: Vec<f64> = plan
        .coverage
        .cells
        .iter()
        .map(|&[w, _, e, _]| e - w)
        .collect();
    assert!(spans.iter().all(|span| *span < 0.05), "{spans:?}");
    assert!(!plan.empty);
}

#[test]
fn a_cluster_costs_one_query_however_many_transformers_it_holds() {
    // A chain of transformers along a road is one site. Fusing to the
    // cluster envelope keeps it one acquisition; the exact rectilinear
    // union of the same chain would be O(n) rectangles, which would turn a
    // district into hundreds of provider round trips — the exact cost this
    // cache exists to avoid.
    let chain: Vec<Extent> = (0..40)
        .map(|step| {
            let west = 30.0 + f64::from(step) * 0.004;
            extent(
                &format!("T{step}"),
                Some([west, -2.0, west + 0.002, -1.998]),
            )
        })
        .collect();
    let plan = plan(&scope(), &buildings(), &policy(), &chain).unwrap();
    assert_eq!(plan.clusters.len(), 1);
    assert_eq!(plan.clusters[0].members.len(), 40);
    assert_eq!(plan.coverage.cells.len(), 1);
    assert!(!plan.clusters[0].isolated);
}

#[test]
fn an_undesigned_transformer_takes_the_isolated_buffer_under_each_rule() {
    let point = [extent("solo", Some([30.0, -2.0, 30.0, -2.0]))];
    let wide = plan(&scope(), &buildings(), &policy(), &point).unwrap();
    assert_eq!(wide.clusters[0].buffer_m, 1_000.0);
    let [w, _, e, _] = wide.coverage.cells[0];
    assert!((2_000.0..2_005.0).contains(&easting(-2.0, w, e)));

    // A designed single transformer separates the two candidate rules:
    // point-only keeps it at 200 m, single-cluster widens it to 1 km.
    let designed = [extent("solo", Some([30.0, -2.0, 30.002, -1.998]))];
    let narrow = plan(&scope(), &buildings(), &policy(), &designed).unwrap();
    assert_eq!(narrow.clusters[0].buffer_m, 200.0);
    let by_cluster = BufferPolicy {
        isolated_rule: IsolatedRule::SingleTransformerCluster,
        ..policy()
    };
    let widened = plan(&scope(), &buildings(), &by_cluster, &designed).unwrap();
    assert_eq!(widened.clusters[0].buffer_m, 1_000.0);
    assert_ne!(narrow.coverage, widened.coverage);
    // The policy that produced a coverage is part of its acquisition
    // identity, so answering the open question is an ordinary extension.
    assert_ne!(narrow.query_digest, widened.query_digest);
}

#[test]
fn a_transformer_without_geometry_is_named_not_guessed() {
    let plan = plan(
        &scope(),
        &buildings(),
        &policy(),
        &[extent("no-geometry", None)],
    )
    .unwrap();
    assert_eq!(plan.skipped, vec!["no-geometry"]);
    assert_eq!(plan.located_count, 0);
    assert!(plan.empty && plan.coverage_geometry.is_none());
}

#[test]
fn held_coverage_answers_a_repeat_and_only_the_delta_is_a_gap() {
    let held = Coverage {
        cells: vec![[0.0, 0.0, 2.0, 2.0]],
    };
    let inside = Coverage {
        cells: vec![[0.5, 0.5, 1.5, 1.5]],
    };
    let repeat = assess(&held, &inside).unwrap();
    assert!(repeat.covered && repeat.gaps.is_empty());

    // A design that grew east asks only for the strip it added.
    let grown = Coverage {
        cells: vec![[0.0, 0.0, 3.0, 2.0]],
    };
    let extended = assess(&held, &grown).unwrap();
    assert!(!extended.covered);
    assert_eq!(extended.gaps.cells, vec![[2.0, 0.0, 3.0, 2.0]]);
    assert_eq!(extended.surplus_count, 0);

    // Folding that acquisition in makes the next identical ask free.
    let after = extend(&held, &extended.gaps).unwrap();
    assert_eq!(after.cells, vec![[0.0, 0.0, 3.0, 2.0]]);
    assert!(assess(&after, &grown).unwrap().covered);
}

#[test]
fn a_deleted_transformer_leaves_surplus_and_destroys_nothing() {
    let held = Coverage {
        cells: vec![[0.0, 0.0, 1.0, 1.0], [5.0, 5.0, 6.0, 6.0]],
    };
    let wanted = Coverage {
        cells: vec![[0.0, 0.0, 1.0, 1.0]],
    };
    let result = assess(&held, &wanted).unwrap();
    assert!(result.covered);
    assert_eq!(result.surplus.cells, vec![[5.0, 5.0, 6.0, 6.0]]);
}

#[test]
fn a_hole_between_clusters_is_never_paid_for_twice() {
    // Four sites around an empty middle. The fused coverage must not become
    // one filled square — the middle was never wanted.
    let ring = Coverage {
        cells: vec![
            [0.0, 0.0, 3.0, 1.0],
            [0.0, 2.0, 3.0, 3.0],
            [0.0, 1.0, 1.0, 2.0],
            [2.0, 1.0, 3.0, 2.0],
        ],
    };
    let merged = extend(&Coverage::default(), &ring).unwrap();
    let centre = Coverage {
        cells: vec![[1.4, 1.4, 1.6, 1.6]],
    };
    let result = assess(&merged, &centre).unwrap();
    assert!(!result.covered);
    assert_eq!(result.gaps.cells, vec![[1.4, 1.4, 1.6, 1.6]]);
}

#[test]
fn extending_exact_coverage_refuses_to_replace_many_cells_with_their_envelope() {
    let held = Coverage {
        cells: (0..MAX_COVERAGE_CELLS)
            .map(|index| {
                let west = index as f64 * 0.002;
                [west, 0.0, west + 0.001, 0.001]
            })
            .collect(),
    };
    let west = MAX_COVERAGE_CELLS as f64 * 0.002;
    let completed = Coverage {
        cells: vec![[west, 0.0, west + 0.001, 0.001]],
    };

    let error = extend(&held, &completed).unwrap_err();
    assert!(error.contains("exact completed coverage"), "{error}");

    // The refusal matters because the envelope would claim an unpaid hole
    // between every pair of cells.
    let hole = Coverage {
        cells: vec![[0.00125, 0.00025, 0.00175, 0.00075]],
    };
    assert!(!assess(&held, &hole).unwrap().covered);
}

#[test]
fn identity_separates_the_room_address_from_the_acquisition() {
    let coverage = Coverage {
        cells: vec![[0.0, 0.0, 1.0, 1.0]],
    };
    let mut fresh = buildings();
    fresh.source_version = "v2".into();
    // A new provider version is freshness, not a different room.
    assert_eq!(
        dataset_key(&scope(), &buildings()),
        dataset_key(&scope(), &fresh)
    );
    assert_ne!(
        query_digest(&scope(), &buildings(), &policy(), &coverage),
        query_digest(&scope(), &fresh, &policy(), &coverage)
    );
    // Another principal, another project, other filters, and a tile-quality
    // extract are all different rooms.
    let other_principal = Scope {
        principal: "uid-2".into(),
        ..scope()
    };
    let other_project = Scope {
        project: "project-b".into(),
        ..scope()
    };
    let lossy = Dataset {
        quality: Quality::TileExtract,
        ..buildings()
    };
    let mut filtered = buildings();
    filtered
        .parameters
        .insert("min_confidence".into(), json!(0.9));
    let mine = dataset_key(&scope(), &buildings());
    for other in [
        dataset_key(&other_principal, &buildings()),
        dataset_key(&other_project, &buildings()),
        dataset_key(&scope(), &lossy),
        dataset_key(&scope(), &filtered),
    ] {
        assert_ne!(mine, other);
    }
    // Two datasets in the same project never share a room, so one can never
    // answer for the other's readiness.
    let contours = Dataset {
        id: "elevation_contours".into(),
        ..buildings()
    };
    assert_ne!(mine, dataset_key(&scope(), &contours));
}

#[test]
fn identity_is_stable_across_parameter_order() {
    let mut reordered = buildings();
    reordered.parameters.clear();
    reordered
        .parameters
        .insert("min_confidence".into(), json!(0.7));
    reordered.parameters.insert("min_area_m2".into(), json!(25));
    assert_eq!(
        dataset_key(&scope(), &buildings()),
        dataset_key(&scope(), &reordered)
    );
}

#[test]
fn requests_are_validated_before_any_geometry_is_trusted() {
    assert!(evaluate(b"{}").is_err());
    let wrong =
        json!({"action":"assess","schema":"ds.other/v1","held":{"cells":[]},"wanted":{"cells":[]}});
    assert!(evaluate(wrong.to_string().as_bytes()).is_err());
    let inverted = json!({"action":"assess","schema":SCHEMA,"held":{"cells":[[2.0,0.0,1.0,1.0]]},"wanted":{"cells":[]}});
    assert!(evaluate(inverted.to_string().as_bytes()).is_err());
    let unprojected = Dataset {
        crs: "EPSG:3857".into(),
        ..buildings()
    };
    assert!(plan(&scope(), &unprojected, &policy(), &[]).is_err());
    assert!(plan(
        &scope(),
        &buildings(),
        &BufferPolicy {
            design_buffer_m: -1.0,
            ..policy()
        },
        &[]
    )
    .is_err());
}

#[test]
fn one_named_rule_governs_both_acquisition_and_the_deletion_that_replaces_it() {
    // The buildings door fetches by intersection, so a footprint crossing
    // the edge of an area belongs to that area. Deletion asks the same
    // question through the same call — there is no second predicate to
    // drift.
    assert_eq!(
        acquisition_selection(BUILDINGS_DATASET_ID, "").unwrap(),
        AreaSelection::Intersects
    );
    let area = json!({"type":"Polygon","coordinates":[[[0.,0.],[1.,0.],[1.,1.],[0.,1.],[0.,0.]]]});
    let crossing = json!({"type":"Feature","properties":{},
            "geometry":{"type":"Polygon","coordinates":[[[0.9,0.4],[1.3,0.4],[1.3,0.6],[0.9,0.6],[0.9,0.4]]]}});
    assert!(acquisition_selects(BUILDINGS_DATASET_ID, "", &crossing, &area).unwrap());
    assert_eq!(
        acquisition_selection("4eebae72digest", REFERENCE_CACHE_PROVIDER).unwrap(),
        AreaSelection::Intersects
    );
    // A cloud dataset's bounded read is ST_INTERSECTS on the cell.
    assert_eq!(
        acquisition_selection("5815decbdigest", CLOUD_READ_PROVIDER).unwrap(),
        AreaSelection::Intersects
    );
    // An unregistered dataset has no rule, and a room may not invent a
    // destructive one for it.
    let refused = acquisition_selection("some_new_dataset", "some-provider").unwrap_err();
    assert!(refused.contains("acquisition_selection"), "{refused}");
    assert!(acquisition_selects("some_new_dataset", "some-provider", &crossing, &area).is_err());
}

#[test]
fn the_capture_is_the_sheets_proportion_scaled_by_the_design_never_a_bare_metre_count() {
    // Butezi: 1.45 km wide, 1.66 km tall. On a landscape sheet the design
    // fills half the frame's width or nine tenths of its height, whichever
    // binds; the frame then shows about 2.9 km × 2.0 km, which is what
    // must be held — not the design plus 200 m.
    let rect = Rect::from_bounds([29.910, -2.522, 29.923, -2.507]).unwrap();
    let mid = -2.5145;
    let size = |r: Rect| {
        (
            (r.e - r.w) * metres_per_degree_longitude(mid),
            (r.n - r.s) * metres_per_degree_latitude(mid),
        )
    };
    let (width_m, height_m) = size(capture(rect, 200.0, &[LANDSCAPE_SHEET]));
    assert!((2_800.0..=3_300.0).contains(&width_m), "width {width_m}");
    assert!((1_900.0..=2_200.0).contains(&height_m), "height {height_m}");
    // Turned, the design takes half the height: about 2.4 km × 3.5 km.
    let (width_m, height_m) = size(capture(rect, 200.0, &[PORTRAIT_SHEET]));
    assert!(
        (2_300.0..=2_600.0).contains(&width_m),
        "portrait width {width_m}"
    );
    assert!(
        (3_300.0..=3_600.0).contains(&height_m),
        "portrait height {height_m}"
    );
    // Both: the union, so whichever sheet the set prints on is filled.
    let held = capture(rect, 200.0, &[LANDSCAPE_SHEET, PORTRAIT_SHEET]);
    let (width_m, height_m) = size(held);
    assert!(
        (2_800.0..=3_300.0).contains(&width_m),
        "both width {width_m}"
    );
    assert!(
        (3_300.0..=3_600.0).contains(&height_m),
        "both height {height_m}"
    );
    // The floor still applies: a metre buffer larger than the sheet wins.
    let floor = capture(rect, 5_000.0, &[LANDSCAPE_SHEET]);
    assert!(floor.w < held.w && floor.e > held.e);
    // No sheet: the metre buffer alone, as before.
    assert_eq!(capture(rect, 200.0, &[]), buffered(rect, 200.0));
    // A bare point is not a design; the plan keeps its isolated radius.
    let point = Rect::from_bounds([30.0, -2.0, 30.0, -2.0]).unwrap();
    assert_eq!(
        capture(point, 1_000.0, &[LANDSCAPE_SHEET]),
        buffered(point, 1_000.0)
    );
    // A policy authored without sheets is the metre buffer alone and
    // reads back exactly as it was given; a seed that prints names its
    // sheets, and those ride along.
    let old: BufferPolicy = serde_json::from_value(
        json!({"design_buffer_m":200.0,"isolated_buffer_m":1000.0,"isolated_rule":"point_only"}),
    )
    .unwrap();
    assert!(old.sheets.is_empty());
    assert_eq!(old, BufferPolicy::default());
    let written = serde_json::to_value(&old).unwrap();
    assert!(written.get("sheets").is_none(), "{written}");
    let printing = BufferPolicy {
        sheets: both_orientations(),
        ..old.clone()
    };
    let written = serde_json::to_value(&printing).unwrap();
    assert_eq!(written["sheets"].as_array().map(Vec::len), Some(2));
    let back: BufferPolicy = serde_json::from_value(written).unwrap();
    assert_eq!(back.sheets, both_orientations());
}

#[test]
fn a_footprint_at_the_providers_boundary_margin_is_selected_and_a_distant_one_is_not() {
    // Seen on the Gisagara buildings room: a 23 m remainder strip, and a
    // footprint the provider returned whose top edge sat a few centimetres
    // below the strip's bottom edge — an upstream (spherical, tolerant)
    // yes and a local (planar, exact) no that refused the acquisition.
    let strip = json!({"type":"Polygon","coordinates":[[
            [29.758594, -2.600046], [29.791722, -2.600046], [29.791722, -2.599841],
            [29.758594, -2.599841], [29.758594, -2.600046]]]});
    let touching = json!({"type":"Feature","properties":{},
            "geometry":{"type":"Polygon","coordinates":[[
                [29.780818, -2.600158], [29.780931, -2.600158], [29.780931, -2.6000463],
                [29.780818, -2.6000463], [29.780818, -2.600158]]]}});
    assert!(
        !crate::reference_cache::selects_area(&touching, &strip, AreaSelection::Intersects)
            .unwrap(),
        "the exact test must still say no, or the margin is untested"
    );
    assert!(acquisition_selects(BUILDINGS_DATASET_ID, "", &touching, &strip).unwrap());
    // Two metres out is beyond any provider margin: not selected.
    let distant = json!({"type":"Feature","properties":{},
            "geometry":{"type":"Polygon","coordinates":[[
                [29.780818, -2.600158], [29.780931, -2.600158], [29.780931, -2.600066],
                [29.780818, -2.600066], [29.780818, -2.600158]]]}});
    assert!(!acquisition_selects(BUILDINGS_DATASET_ID, "", &distant, &strip).unwrap());
    // The margin is a property of the intersects rule, not of buildings:
    // a centre rule ignores it.
    assert!(!crate::reference_cache::selects_area(
        &touching,
        &strip,
        AreaSelection::CentroidInside
    )
    .unwrap());
}

/// 2026-09-27 review: a road an earlier national seed held was deleted by
/// a later tile its bounds cross into, and the index never handed it back.
#[test]
fn the_reference_cache_selects_exactly_what_its_index_reads() {
    let tile = json!({"type":"Polygon","coordinates":[[
            [30.012, -2.008], [30.02, -2.008], [30.02, -2.0], [30.012, -2.0], [30.012, -2.008]]]});
    // Leaning south-east: its bounds cover the tile, the road passes 300 m
    // south-west of it.
    let road = json!({"type":"Feature","properties":{},
            "geometry":{"type":"LineString","coordinates":[[30.0, -2.0], [30.02, -2.02]]}});
    // Half a metre west of the tile, and one ending on its edge.
    let near = json!({"type":"Feature","properties":{},
            "geometry":{"type":"Point","coordinates":[30.011995, -2.004]}});
    let touching = json!({"type":"Feature","properties":{},
            "geometry":{"type":"LineString","coordinates":[[30.0, -2.004], [30.012, -2.004]]}});
    let national = "a".repeat(64);
    for (feature, index_reads) in [(&road, false), (&near, false), (&touching, true)] {
        assert_eq!(
            crate::reference_cache::intersects_area(feature, &tile).unwrap(),
            index_reads
        );
        assert_eq!(
            acquisition_selects(&national, REFERENCE_CACHE_PROVIDER, feature, &tile).unwrap(),
            index_reads,
            "{feature}"
        );
    }
    // A remote provider keeps its margin, grown bounds and all.
    for feature in [&road, &near, &touching] {
        assert!(acquisition_selects(&national, CLOUD_READ_PROVIDER, feature, &tile).unwrap());
    }
}

#[test]
fn an_oversized_or_malformed_source_receipt_is_refused_not_trimmed() {
    assert!(validate_source_receipt(&json!({"dataset_version": "v3"})).is_ok());
    assert!(validate_source_receipt(&json!("v3")).is_err());
    let huge = json!({"body": "x".repeat(MAX_SOURCE_RECEIPT_BYTES)});
    let error = validate_source_receipt(&huge).unwrap_err();
    assert!(error.contains("over the"), "{error}");
}

#[test]
fn an_abandoned_acquisition_expires_at_a_bounded_age_and_is_never_pending() {
    let now = 10 * ACQUISITION_TICKET_TTL_MS;
    assert_eq!(
        acquisition_state("requested", now - ACQUISITION_TICKET_TTL_MS, now).unwrap(),
        "requested"
    );
    assert_eq!(
        acquisition_state("requested", now - ACQUISITION_TICKET_TTL_MS - 1, now).unwrap(),
        "expired"
    );
    // Expiry is not failure: nothing went wrong and nothing was written.
    assert_eq!(acquisition_state("failed", 0, now).unwrap(), "failed");
    assert_eq!(acquisition_state("completed", 0, now).unwrap(), "completed");
    assert!(acquisition_state("half-done", 0, now).is_err());
    // A clock that moves backwards must not resurrect an old ticket as
    // fresh work, nor overflow.
    assert!(!acquisition_expired(now, 0));
    assert!(!acquisition_expired(i64::MIN, i64::MIN));
}

#[test]
fn freshness_reports_obsolete_coverage_and_never_removes_it() {
    let held = |version: &str, cells: Vec<[f64; 4]>| HeldVersion {
        source_version: version.into(),
        coverage: Coverage { cells },
    };
    let rows = [
        held("v1", vec![[0.0, 0.0, 1.0, 1.0]]),
        held("v2", vec![[2.0, 0.0, 3.0, 1.0]]),
        held("", vec![[5.0, 0.0, 6.0, 1.0]]),
    ];
    let report = freshness("v2", &rows).unwrap();
    assert!(!report.fresh);
    assert_eq!(report.stale.cells, vec![[0.0, 0.0, 1.0, 1.0]]);
    assert_eq!(report.unattributed.cells, vec![[5.0, 0.0, 6.0, 1.0]]);
    assert_eq!(report.versions, vec!["v1".to_string(), "v2".to_string()]);
    assert_eq!(
        report.stale_geometry.as_ref().unwrap()["type"],
        "MultiPolygon"
    );

    // Coverage that is stale is still coverage: this reports it, and the
    // caller still holds every cell it held before.
    let all = Coverage {
        cells: vec![[0.0, 0.0, 1.0, 1.0], [2.0, 0.0, 3.0, 1.0]],
    };
    assert!(
        assess(
            &all,
            &Coverage {
                cells: vec![[0.0, 0.0, 1.0, 1.0]]
            }
        )
        .unwrap()
        .covered
    );

    // No known current version is no evidence of obsolescence.
    assert!(freshness("", &rows).unwrap().fresh);
    // And once everything is re-acquired under v2, nothing is stale.
    let refreshed = [held("v2", vec![[0.0, 0.0, 1.0, 1.0], [2.0, 0.0, 3.0, 1.0]])];
    assert!(freshness("v2", &refreshed).unwrap().fresh);
}

#[test]
fn the_wire_surface_answers_a_full_round_trip() {
    let request = json!({
        "action": "plan", "schema": SCHEMA,
        "scope": {"principal": "uid-1", "project": "project-a"},
        "dataset": {"id": "google_open_buildings", "provider": "ds-brain:query_print_context",
            "quality": "source", "parameters": {"min_area_m2": 25}},
        "policy": {"design_buffer_m": 500.0, "isolated_buffer_m": 1000.0,
            "isolated_rule": "point_only", "provisional": true},
        "extents": [{"name": "T1", "bounds": [30.0, -2.0, 30.002, -1.998]}]
    });
    let planned: Value =
        serde_json::from_str(&evaluate(request.to_string().as_bytes()).unwrap()).unwrap();
    assert_eq!(planned["schema"], SCHEMA);
    assert_eq!(planned["coverage_geometry"]["type"], "MultiPolygon");
    assert_eq!(planned["query_digest"].as_str().unwrap().len(), 64);
    let assessed = json!({"action":"assess","schema":SCHEMA,
            "held": planned["coverage"], "wanted": planned["coverage"]});
    let result: Value =
        serde_json::from_str(&evaluate(assessed.to_string().as_bytes()).unwrap()).unwrap();
    assert_eq!(result["covered"], true);
    assert_eq!(result["gap_count"], 0);
}
fn candidate(id: &str, label: &str, seeded: bool) -> SeedCandidate {
    SeedCandidate {
        id: id.into(),
        label: label.into(),
        seeded,
    }
}

/// First use: a machine that holds nothing still seeds what the project
/// declares. This is the defect the command shipped with — an unqualified
/// seed could only ever refresh datasets already seeded, so on a fresh
/// install it refused with "this project holds no dataset yet", which is
/// the exact state the operator was trying to leave.
#[test]
fn a_seed_that_names_no_dataset_acquires_what_the_project_declares() {
    let declared = [
        candidate("google_open_buildings", "Building footprints", false),
        candidate("rwanda_roads", "Roads", false),
    ];
    assert_eq!(
        seed_targets(&declared, &[], "").unwrap(),
        // Ordered by label, so two machines seed in the same order.
        vec!["google_open_buildings", "rwanda_roads"]
    );
}

/// What the project declares UNION what it already holds — and a held row
/// that was never seeded is not a holding.
#[test]
fn a_seed_keeps_refreshing_a_dataset_added_outside_the_declaration() {
    let declared = [candidate(
        "google_open_buildings",
        "Building footprints",
        false,
    )];
    let held = [
        candidate("rwanda_upi_parcels", "Parcels", true),
        candidate("rwanda_rivers", "Rivers", false),
    ];
    assert_eq!(
        seed_targets(&declared, &held, "").unwrap(),
        // Declared first, then what is held beside it; never the row that
        // has no room, which would be introducing a dataset by accident.
        vec!["google_open_buildings", "rwanda_upi_parcels"]
    );
}

/// A dataset in both lists is one target, and it keeps its declared place.
#[test]
fn a_declared_dataset_that_is_also_held_is_seeded_once() {
    let declared = [
        candidate("rwanda_roads", "Roads", false),
        candidate("google_open_buildings", "Building footprints", false),
    ];
    let held = [candidate("rwanda_roads", "Roads", true)];
    assert_eq!(
        seed_targets(&declared, &held, "").unwrap(),
        vec!["google_open_buildings", "rwanda_roads"]
    );
}

/// Naming one dataset is the act of adding one the project has not
/// declared, so it is the whole answer.
#[test]
fn an_explicit_dataset_is_the_only_target() {
    let declared = [candidate(
        "google_open_buildings",
        "Building footprints",
        false,
    )];
    assert_eq!(
        seed_targets(&declared, &[], "  rwanda_upi_parcels ").unwrap(),
        vec!["rwanda_upi_parcels"]
    );
    assert!(seed_targets(&[], &[], "  ").is_err());
}

/// Nothing declared and nothing held is a typed refusal, not an empty
/// success: a run that acquired nothing must never read as a seed.
#[test]
fn a_seed_with_nothing_to_act_on_refuses_by_name() {
    assert_eq!(
        seed_targets(&[], &[candidate("rwanda_roads", "Roads", false)], "").unwrap_err(),
        SEED_SCOPE_REFUSAL
    );
    let many: Vec<SeedCandidate> = (0..MAX_SEED_CANDIDATES + 1)
        .map(|index| candidate(&format!("d{index}"), "", false))
        .collect();
    assert!(seed_targets(&many, &[], "")
        .unwrap_err()
        .contains("more than"));
    assert!(seed_targets(&[candidate("", "", false)], &[], "").is_err());
}

/// The same answers through the request door the WASM export and `ds`
/// reach.
#[test]
fn the_seed_target_action_answers_through_the_request_door() {
    let request = json!({"action":"seed_targets","schema":SCHEMA,
            "declared":[{"id":"google_open_buildings","label":"Building footprints"}],
            "held":[{"id":"rwanda_upi_parcels","label":"Parcels","seeded":true}]});
    let value: Value =
        serde_json::from_str(&evaluate(request.to_string().as_bytes()).unwrap()).unwrap();
    assert_eq!(value["schema"], SCHEMA);
    assert_eq!(
        value["targets"],
        json!(["google_open_buildings", "rwanda_upi_parcels"])
    );
    let empty = json!({"action":"seed_targets","schema":SCHEMA});
    assert_eq!(
        evaluate(empty.to_string().as_bytes()).unwrap_err(),
        SEED_SCOPE_REFUSAL
    );
}

#[test]
fn a_shared_seed_holds_the_union_of_design_and_route_extents_within_the_tolerance() {
    // Two LV designs 600 m apart (one cluster once widened), one MV route
    // segment between them and a transformer that is still a bare point.
    let extents = [
        extent("lv-a", Some([30.000, -2.000, 30.002, -1.998])),
        extent("lv-b", Some([30.0074, -2.000, 30.0094, -1.998])),
        extent("mv-route", Some([30.002, -1.9995, 30.0074, -1.9985])),
        extent("bare", Some([30.004, -2.006, 30.004, -2.006])),
    ];
    let shared = shared_acquisition_policy(DEFAULT_CONTEXT_TOLERANCE_M).unwrap();
    let planned = plan(&scope(), &buildings(), &shared, &extents).unwrap();
    // One cell per member, each exactly its extent widened by the
    // tolerance: no envelope between them and no sheet frame around them.
    let mut wanted: Vec<[f64; 4]> = extents
        .iter()
        .map(|extent| {
            let rect = Rect::from_bounds(extent.bounds.unwrap()).unwrap();
            buffered(rect, DEFAULT_CONTEXT_TOLERANCE_M).bounds()
        })
        .collect();
    wanted.sort_by(|a, b| a.partial_cmp(b).unwrap());
    assert_eq!(planned.coverage.cells, wanted);
    assert!(planned.clusters.iter().all(|cluster| cluster.split));
    // The bare transformer is widened by the tolerance too, not 1 km.
    let [w, _, e, _] = buffered(
        Rect::from_bounds([30.004, -2.006, 30.004, -2.006]).unwrap(),
        DEFAULT_CONTEXT_TOLERANCE_M,
    )
    .bounds();
    assert!((400.0..402.0).contains(&easting(-2.006, w, e)));
    // The cluster-envelope plan of the same extents asks for strictly more
    // ground: that over-fetch is what the shared seed never acquires.
    let enveloped = plan(&scope(), &buildings(), &BufferPolicy::default(), &extents).unwrap();
    let held = Coverage {
        cells: planned.coverage.cells.clone(),
    };
    assert!(!assess(&held, &enveloped.coverage).unwrap().covered);
    assert!(
        assess(&enveloped.coverage, &planned.coverage)
            .unwrap()
            .covered
    );
}

#[test]
fn the_context_tolerance_is_a_project_setting_with_a_published_default() {
    let settings = |rows: Value| json!({"sheets": {"project_settings": rows}});
    assert_eq!(context_tolerance_m(&json!({})).unwrap(), 200.0);
    assert_eq!(
        context_tolerance_m(&settings(
            json!([{"parameter":"report_locale","value":"fr"}])
        ))
        .unwrap(),
        DEFAULT_CONTEXT_TOLERANCE_M
    );
    for value in [json!(350), json!("350"), json!(350.0)] {
        let authored =
            settings(json!([{"parameter": CONTEXT_TOLERANCE_PARAMETER, "value": value}]));
        assert_eq!(context_tolerance_m(&authored).unwrap(), 350.0);
    }
    // Malformed never silently becomes the default.
    for value in [
        json!(-1),
        json!(5_001),
        json!("wide"),
        json!(null),
        json!(true),
    ] {
        let authored =
            settings(json!([{"parameter": CONTEXT_TOLERANCE_PARAMETER, "value": value}]));
        let error = context_tolerance_m(&authored).unwrap_err();
        assert!(
            error.starts_with("project_context_tolerance_invalid"),
            "{error}"
        );
    }
    let twice = settings(json!([
        {"parameter": CONTEXT_TOLERANCE_PARAMETER, "value": 100},
        {"parameter": CONTEXT_TOLERANCE_PARAMETER, "value": 300},
    ]));
    assert!(context_tolerance_m(&twice).is_err());
    assert!(is_governed_parameter(CONTEXT_TOLERANCE_PARAMETER));

    let policy = shared_acquisition_policy(350.0).unwrap();
    assert_eq!(
        (policy.design_buffer_m, policy.isolated_buffer_m),
        (350.0, 350.0)
    );
    assert!(policy.sheets.is_empty());
    assert_eq!(policy.area, AcquisitionArea::BufferedExtents);
    assert!(shared_acquisition_policy(f64::NAN).is_err());
}

#[test]
fn the_acquisition_area_rides_the_wire_only_when_it_is_not_the_default() {
    let written = serde_json::to_value(BufferPolicy::default()).unwrap();
    assert!(written.get("area").is_none(), "{written}");
    let shared = shared_acquisition_policy(200.0).unwrap();
    let written = serde_json::to_value(&shared).unwrap();
    assert_eq!(written["area"], "buffered_extents");
    let back: BufferPolicy = serde_json::from_value(written).unwrap();
    assert_eq!(back, shared);
}

#[test]
fn a_design_reaching_a_zero_coordinate_is_named_and_the_rest_still_plans() {
    // A vertex left at NIX (0, 0) lands near (23.66, -44.97): its design's
    // rectangle spans half a continent. It is named, never acquired, and
    // never stops the project's real designs from planning.
    let extents = [
        extent("lv-real", Some([29.70, -2.70, 29.702, -2.698])),
        extent(
            "lv-stray",
            Some([23.659740281, -44.965635885, 29.767231262, -2.855510961]),
        ),
    ];
    for policy in [policy(), shared_acquisition_policy(200.0).unwrap()] {
        let planned = plan(&scope(), &buildings(), &policy, &extents).unwrap();
        assert_eq!(planned.oversized, vec!["lv-stray"]);
        assert_eq!(planned.located_count, 1);
        assert_eq!(planned.coverage.cells.len(), 1);
        let [w, s, e, n] = planned.coverage.cells[0];
        assert!(
            w > 29.6 && e < 29.8 && s > -2.8 && n < -2.6,
            "{w} {s} {e} {n}"
        );
        let written = serde_json::to_value(&planned).unwrap();
        assert_eq!(written["oversized"], json!(["lv-stray"]));
    }
    // A plan with nothing oversized keeps its reply shape.
    let clean = plan(&scope(), &buildings(), &policy(), &extents[..1]).unwrap();
    assert!(serde_json::to_value(&clean)
        .unwrap()
        .get("oversized")
        .is_none());
}
