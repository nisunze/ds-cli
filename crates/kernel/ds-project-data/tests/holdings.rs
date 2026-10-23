//! The crate's promises, against real rooms and a real bundle index under a
//! temporary root, with a scripted provider that counts what it was asked.

use ds_command_kernel::printing::{PrintContextLayer, PrintContextSource};
use ds_command_kernel::project_dataset_cache::{buffer_policy, Dataset, Scope};
use ds_command_kernel::reference_cache::Identity;
use ds_command_kernel::report_export::{sha256_hex, PRINT_CONTEXT_SCHEMA};
use ds_project_data::catalog::ReferenceResource;
use ds_project_data::{
    buildings_dataset, catalog_dataset, contours_dataset, declared, design_extents, ensure,
    install_bundle, read_print_context, Acquisition, BundleReceipt, ContourSettings,
    DeclaredSource, Failure, Hosts, Mode, Provider,
};
use flate2::write::GzEncoder;
use flate2::Compression;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::io::Write;
use std::path::Path;

fn scope() -> Scope {
    Scope {
        principal: "uid-1".into(),
        project: "project-a".into(),
    }
}

/// A small designed transformer near 30°E 2°S: two poles 100 m apart.
fn design() -> Value {
    json!({
        "lv_poles": {"type": "FeatureCollection", "features": [
            {"type": "Feature", "properties": {"name": "P1"}, "geometry": {"type": "Point", "coordinates": [30.0, -2.0]}},
            {"type": "Feature", "properties": {"name": "P2"}, "geometry": {"type": "Point", "coordinates": [30.001, -2.0009]}}
        ]}
    })
}

fn footprint(index: usize, x: f64, y: f64) -> Value {
    let d = 0.00005;
    json!({"type": "Feature", "properties": {"id": format!("b{index}"), "area_in_meters": 40.0},
        "geometry": {"type": "Polygon", "coordinates": [[[x, y], [x + d, y], [x + d, y + d], [x, y + d], [x, y]]]}})
}

fn contour(index: usize, kind: &str, y: f64) -> Value {
    json!({"type": "Feature", "properties": {"id": format!("c{index}"), "contour_kind": kind, "elevation": 1500 + index},
        "geometry": {"type": "LineString", "coordinates": [[29.9995, y], [30.0015, y]]}})
}

fn long_narrow_design() -> Value {
    json!({"lv_lines": {"type":"FeatureCollection", "features":[
        {"type":"Feature", "properties":{}, "geometry":{"type":"LineString",
            "coordinates":[[30.0,-2.0],[30.02,-1.9999]]}}
    ]}})
}

/// Answers every area with three footprints inside it, and counts calls.
struct Footprints {
    calls: usize,
    fail_first: bool,
}

impl Provider for Footprints {
    fn acquire(&mut self, dataset: &Dataset, area: &Value) -> Result<Acquisition, Failure> {
        self.calls += 1;
        if self.fail_first && self.calls == 1 {
            return Err(Failure::ProviderUnavailable("provider is down".into()));
        }
        let ring = &area["coordinates"][0];
        let (w, s) = (ring[0][0].as_f64().unwrap(), ring[0][1].as_f64().unwrap());
        let (e, n) = (ring[2][0].as_f64().unwrap(), ring[2][1].as_f64().unwrap());
        let (cx, cy) = ((w + e) / 2.0, (s + n) / 2.0);
        let features = if dataset.id == "elevation_contours" {
            vec![
                contour(1, "index", cy),
                contour(2, "intermediate", cy + 0.0001),
                contour(3, "intermediate", cy - 0.0001),
            ]
        } else {
            vec![
                footprint(1, cx, cy),
                footprint(2, cx + 0.0002, cy),
                footprint(3, cx, cy + 0.0002),
            ]
        };
        Ok(Acquisition {
            features,
            source_version: format!("{}-v1", dataset.id),
            source_receipt: json!({"provider": "scripted", "calls": self.calls}),
        })
    }
}

fn no_fetch(_: &BundleReceipt, _: &Path) -> Result<(), String> {
    Err("no download expected in this test".into())
}

#[test]
fn dataset_documents_are_the_desktops() {
    let buildings = serde_json::to_value(buildings_dataset()).unwrap();
    assert_eq!(
        buildings,
        json!({"id": "google_open_buildings", "provider": "ds-brain:query_print_context", "quality": "source",
            "parameters": {"context_kind": "google_open_buildings"}, "source_version": "", "crs": "EPSG:4326"})
    );
    let resource = ReferenceResource {
        id: "a".repeat(64),
        version: "b".repeat(64),
        label: "Roads".into(),
        layer: "roads".into(),
        country: "Rwanda".into(),
        kind: "bigquery".into(),
        active: true,
        ..Default::default()
    };
    let catalog = serde_json::to_value(catalog_dataset(&resource)).unwrap();
    assert_eq!(catalog["provider"], "desktop:reference_resource_cache");
    assert_eq!(
        catalog["parameters"],
        json!({"resource_id": "a".repeat(64), "layer": "roads", "country": "Rwanda"})
    );
    let contours = contours_dataset(ContourSettings {
        minor_interval_m: 10,
        index_interval_m: 50,
        sample_spacing_m: 10.0,
    });
    assert_eq!(contours.id, "elevation_contours");
    assert_eq!(contours.parameters["index_interval_m"], json!(50));
}

#[test]
fn a_project_declares_the_kernels_defaults_and_the_derived_sources() {
    let roads = ReferenceResource {
        id: "a".repeat(64),
        version: "b".repeat(64),
        label: "Roads".into(),
        layer: "roads".into(),
        country: "Rwanda".into(),
        kind: "bigquery".into(),
        active: true,
        download_url: Some("https://storage.googleapis.com/bucket/roads.geojsonl.gz".into()),
        bundle_sha256: Some("c".repeat(64)),
        expanded_sha256: Some("d".repeat(64)),
        download_bytes: Some(10),
        expanded_bytes: Some(20),
        bundle_features: Some(3),
        bundle_built_at: Some(1),
        ..Default::default()
    };
    let parcels = ReferenceResource {
        id: "e".repeat(64),
        layer: "rwanda_upi_parcels".into(),
        label: "Parcels".into(),
        ..roads.clone()
    };
    let unpublished = ReferenceResource {
        id: "f".repeat(64),
        layer: "rivers".into(),
        label: "Rivers".into(),
        download_url: None,
        bundle_sha256: None,
        expanded_sha256: None,
        download_bytes: None,
        expanded_bytes: None,
        bundle_features: None,
        bundle_built_at: None,
        ..roads.clone()
    };
    let parcels_id = parcels.id.clone();
    let unpublished_id = unpublished.id.clone();
    let declared = declared(&[roads.clone(), parcels, unpublished]).expect("declaration");
    let ids: Vec<&str> = declared.iter().map(|d| d.dataset.id.as_str()).collect();
    assert!(ids.contains(&"google_open_buildings"));
    assert!(ids.contains(&"elevation_contours"));
    assert!(
        ids.contains(&roads.id.as_str()),
        "a downloadable default-on layer is declared"
    );
    assert!(
        !ids.contains(&parcels_id.as_str()),
        "parcels are excluded by the kernel"
    );
    assert!(
        !ids.contains(&unpublished_id.as_str()),
        "an unpublished layer is not declared"
    );
    assert!(!ids.contains(&"project_dsgrid_mv"), "MV has no room");
}

#[test]
fn ensure_reads_never_acquire_and_never_create_a_room() {
    let root = tempfile::tempdir().unwrap();
    let extents = design_extents([("tx", &design())]).unwrap();
    let policy = buffer_policy(&[200.0]).unwrap();
    let outcome = ensure(
        root.path(),
        &scope(),
        &buildings_dataset(),
        &DeclaredSource::Buildings,
        &policy,
        &extents,
        &[],
        Mode::Read,
    )
    .expect("a read answers");
    assert!(!outcome.covered);
    assert_eq!(outcome.acquired, 0);
    assert_eq!(outcome.status["exists"], json!(false));
    assert!(
        !root.path().join("project-dataset-cache").exists(),
        "a read creates nothing on disk"
    );
}

#[test]
fn a_second_unchanged_seed_acquires_nothing() {
    let root = tempfile::tempdir().unwrap();
    let extents = design_extents([("tx", &design())]).unwrap();
    let policy = buffer_policy(&[200.0]).unwrap();
    let mut provider = Footprints {
        calls: 0,
        fail_first: false,
    };
    let mut fetch = no_fetch;
    let first = {
        let mut hosts = Hosts {
            provider: &mut provider,
            fetch: &mut fetch,
        };
        ensure(
            root.path(),
            &scope(),
            &buildings_dataset(),
            &DeclaredSource::Buildings,
            &policy,
            &extents,
            &[],
            Mode::Acquire(&mut hosts),
        )
        .unwrap()
    };
    assert!(first.covered);
    assert!(first.acquired >= 1);
    let calls_after_first = provider.calls;
    assert_eq!(calls_after_first, first.acquired);
    assert_eq!(first.status["ready"], json!(true));
    assert!(first.features >= 3);
    let second = {
        let mut hosts = Hosts {
            provider: &mut provider,
            fetch: &mut fetch,
        };
        ensure(
            root.path(),
            &scope(),
            &buildings_dataset(),
            &DeclaredSource::Buildings,
            &policy,
            &extents,
            &[],
            Mode::Acquire(&mut hosts),
        )
        .unwrap()
    };
    assert!(second.covered);
    assert_eq!(second.acquired, 0, "held coverage answers the plan");
    assert_eq!(
        provider.calls, calls_after_first,
        "no provider call on a warm seed"
    );
    assert_eq!(second.status["completed"], first.status["completed"]);
    // And a plain read now answers covered.
    let read = ensure(
        root.path(),
        &scope(),
        &buildings_dataset(),
        &DeclaredSource::Buildings,
        &policy,
        &extents,
        &[],
        Mode::Read,
    )
    .unwrap();
    assert!(read.covered);
}

#[test]
fn a_failed_cell_never_reads_ready() {
    let root = tempfile::tempdir().unwrap();
    let extents = design_extents([("tx", &design())]).unwrap();
    let policy = buffer_policy(&[200.0]).unwrap();
    let mut provider = Footprints {
        calls: 0,
        fail_first: true,
    };
    let mut fetch = no_fetch;
    let failed = {
        let mut hosts = Hosts {
            provider: &mut provider,
            fetch: &mut fetch,
        };
        ensure(
            root.path(),
            &scope(),
            &buildings_dataset(),
            &DeclaredSource::Buildings,
            &policy,
            &extents,
            &[],
            Mode::Acquire(&mut hosts),
        )
    };
    let error = failed.expect_err("the provider failure is reported");
    assert_eq!(error.code(), "dataset_provider_unavailable");
    let read = ensure(
        root.path(),
        &scope(),
        &buildings_dataset(),
        &DeclaredSource::Buildings,
        &policy,
        &extents,
        &[],
        Mode::Read,
    )
    .unwrap();
    assert!(!read.covered);
    assert_eq!(read.status["ready"], json!(false));
    assert_eq!(read.status["last_error"], json!("provider is down"));
    assert_eq!(read.status["feature_count"], json!(0));
    // The retry completes what the failure left open.
    let retried = {
        let mut hosts = Hosts {
            provider: &mut provider,
            fetch: &mut fetch,
        };
        ensure(
            root.path(),
            &scope(),
            &buildings_dataset(),
            &DeclaredSource::Buildings,
            &policy,
            &extents,
            &[],
            Mode::Acquire(&mut hosts),
        )
        .unwrap()
    };
    assert!(retried.covered);
    assert_eq!(retried.status["ready"], json!(true));
}

/// A tiny national bundle: GeoJSONL, gzip, with every digest the catalogue
/// would publish for it.
fn bundle(features: &[Value]) -> (Vec<u8>, BundleReceipt) {
    let mut expanded = Vec::new();
    for feature in features {
        expanded.extend(serde_json::to_vec(feature).unwrap());
        expanded.push(b'\n');
    }
    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(&expanded).unwrap();
    let compressed = encoder.finish().unwrap();
    let receipt = BundleReceipt {
        identity: Identity {
            resource_id: "1".repeat(64),
            version: "2".repeat(64),
        },
        url: "https://storage.googleapis.com/bucket/roads.geojsonl.gz".into(),
        bundle_sha256: format!("{:x}", Sha256::digest(&compressed)),
        compressed_bytes: compressed.len() as u64,
        expanded_sha256: format!("{:x}", Sha256::digest(&expanded)),
        expanded_bytes: expanded.len() as u64,
        feature_count: features.len() as u64,
        label: "Roads".into(),
    };
    (compressed, receipt)
}

fn road(index: usize, y: f64) -> Value {
    json!({"type": "Feature", "properties": {"id": format!("r{index}"), "name": "RN1"},
        "geometry": {"type": "LineString", "coordinates": [[29.99, y], [30.01, y]]}})
}

#[test]
fn a_bundle_carrying_a_geometry_ds_does_not_admit_is_never_indexed() {
    let root = tempfile::tempdir().unwrap();
    let inverted = json!({"type": "Feature", "properties": {"id": "lake"},
        "geometry": {"type": "Polygon", "coordinates": [
            [[180, 90], [-180, 90], [-180, -90], [180, -90], [180, 90]],
            [[30, -2], [30.1, -2], [30.1, -2.1], [30, -2.1], [30, -2]]]}});
    let (compressed, receipt) = bundle(&[road(1, -2.0), inverted, road(3, -1.9995)]);
    let mut fetch = move |_: &BundleReceipt, dest: &Path| -> Result<(), String> {
        std::fs::write(dest, &compressed).map_err(|e| e.to_string())
    };
    let refused = install_bundle(root.path(), &receipt, &mut fetch).expect_err("refused");
    assert_eq!(refused.code(), "reference_bundle_unavailable");
    assert!(
        refused.message().starts_with(
            "bundle_invalid: feature 2 polygon_covers_hemisphere: Polygon ring 0 spans 360°"
        ),
        "{}",
        refused.message()
    );
    assert!(
        refused
            .message()
            .ends_with("; the Roads bundle was not installed"),
        "{}",
        refused.message()
    );
    assert!(!ds_project_data::bundle::installed(root.path(), &receipt.identity).unwrap());
}

#[test]
fn a_bundle_installs_only_when_its_bytes_are_the_published_ones() {
    let root = tempfile::tempdir().unwrap();
    let (compressed, receipt) = bundle(&[road(1, -2.0), road(2, -2.0005), road(3, -1.9995)]);
    let mut wrong = receipt.clone();
    wrong.bundle_sha256 = "9".repeat(64);
    let bytes = compressed.clone();
    let mut fetch = move |_: &BundleReceipt, dest: &Path| -> Result<(), String> {
        std::fs::write(dest, &bytes).map_err(|e| e.to_string())
    };
    let refused = install_bundle(root.path(), &wrong, &mut fetch).expect_err("digest mismatch");
    assert_eq!(refused.code(), "reference_bundle_unavailable");
    assert!(!ds_project_data::bundle::installed(root.path(), &receipt.identity).unwrap());
    let status = install_bundle(root.path(), &receipt, &mut fetch).expect("install");
    assert_eq!(status["ready"], json!(true));
    assert_eq!(status["features"], json!(3));
    assert!(ds_project_data::bundle::installed(root.path(), &receipt.identity).unwrap());
    // A second install is a status read, not a download.
    let mut never =
        |_: &BundleReceipt, _: &Path| -> Result<(), String> { Err("fetched again".into()) };
    assert_eq!(
        install_bundle(root.path(), &receipt, &mut never).unwrap()["ready"],
        json!(true)
    );
}

fn roads_resource(receipt: &BundleReceipt) -> ReferenceResource {
    ReferenceResource {
        id: receipt.identity.resource_id.clone(),
        version: receipt.identity.version.clone(),
        label: "Roads".into(),
        layer: "roads".into(),
        country: "Rwanda".into(),
        kind: "bigquery".into(),
        active: true,
        download_url: Some(receipt.url.clone()),
        bundle_sha256: Some(receipt.bundle_sha256.clone()),
        expanded_sha256: Some(receipt.expanded_sha256.clone()),
        download_bytes: Some(receipt.compressed_bytes),
        expanded_bytes: Some(receipt.expanded_bytes),
        bundle_features: Some(receipt.feature_count),
        bundle_built_at: Some(1),
        ..Default::default()
    }
}

#[test]
fn a_catalogue_subset_is_extracted_from_the_installed_bundle_and_printed() {
    let root = tempfile::tempdir().unwrap();
    let (compressed, receipt) = bundle(&[road(1, -2.0), road(2, -2.0005), road(3, -1.0)]);
    let resource = roads_resource(&receipt);
    let mut provider = Footprints {
        calls: 0,
        fail_first: false,
    };
    let bytes = compressed.clone();
    let mut fetch = move |_: &BundleReceipt, dest: &Path| -> Result<(), String> {
        std::fs::write(dest, &bytes).map_err(|e| e.to_string())
    };
    let contexts = vec![
        PrintContextLayer {
            id: "roads".into(),
            label: "Roads".into(),
            source: PrintContextSource::Catalog {
                country: "Rwanda".into(),
                layer: "roads".into(),
            },
        },
        PrintContextLayer {
            id: "buildings_context".into(),
            label: "Building footprints".into(),
            source: PrintContextSource::GoogleOpenBuildings {
                buffer_m: 200.0,
                min_area_m2: 25.0,
                min_confidence: 0.7,
            },
        },
        PrintContextLayer {
            id: "elevation_contours".into(),
            label: "Elevation contours".into(),
            source: PrintContextSource::ElevationContours {
                buffer_m: 200.0,
                minor_interval_m: 10,
                index_interval_m: 50,
                sample_spacing_m: 10.0,
            },
        },
        PrintContextLayer {
            id: "project_dsgrid_mv".into(),
            label: "Proposed MV".into(),
            source: PrintContextSource::ProjectDsgridMv {
                buffer_m: 200.0,
                corridor: None,
            },
        },
    ];
    // A read before any seed: nothing held, everything named, nothing fetched.
    let cold = read_print_context(
        root.path(),
        &scope(),
        "tx",
        &design(),
        &contexts,
        std::slice::from_ref(&resource),
        Mode::Read,
    )
    .unwrap();
    assert!(cold.document.is_none());
    assert_eq!(provider.calls, 0);
    assert!(cold
        .omitted
        .iter()
        .any(|o| o.layer == "roads" && o.reason.starts_with("project_dataset_not_held: ")));
    assert!(cold
        .omitted
        .iter()
        .any(|o| o.layer == "dsgrid_mv_lines" && o.reason == "desktop_only"));
    assert!(cold
        .warnings
        .iter()
        .any(|w| w.contains("project-cache seed")));
    // The first print seeds: the bundle is installed, the rooms acquired.
    let seeded = {
        let mut hosts = Hosts {
            provider: &mut provider,
            fetch: &mut fetch,
        };
        read_print_context(
            root.path(),
            &scope(),
            "tx",
            &design(),
            &contexts,
            std::slice::from_ref(&resource),
            Mode::Acquire(&mut hosts),
        )
        .unwrap()
    };
    assert!(!seeded.reused);
    assert_eq!(
        seeded.layers,
        vec![
            "buildings_context",
            "contours_index",
            "contours_intermediate",
            "roads"
        ]
    );
    let document: Value = serde_json::from_slice(seeded.document.as_deref().unwrap()).unwrap();
    assert_eq!(document["schema"], PRINT_CONTEXT_SCHEMA);
    assert_eq!(document["coverage"]["type"], "MultiPolygon");
    assert_eq!(
        document["layers"]["roads"]["features"]
            .as_array()
            .unwrap()
            .len(),
        2,
        "the far road is outside the design's coverage"
    );
    assert_eq!(
        document["layers"]["contours_index"]["features"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        document["layers"]["contours_intermediate"]["features"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert!(document["sources"]
        .as_array()
        .unwrap()
        .iter()
        .all(|s| s["layer"].is_string()));
    assert_eq!(
        seeded.sha256.as_deref(),
        Some(sha256_hex(seeded.document.as_deref().unwrap()).as_str())
    );
    let calls = provider.calls;
    // The second print reuses every room: same bytes, no provider call.
    let warm = read_print_context(
        root.path(),
        &scope(),
        "tx",
        &design(),
        &contexts,
        &[resource],
        Mode::Read,
    )
    .unwrap();
    assert!(warm.reused);
    assert_eq!(provider.calls, calls);
    assert_eq!(warm.document, seeded.document);
    assert_eq!(warm.sha256, seeded.sha256);
}

#[test]
fn contour_print_context_covers_the_sheet_around_a_long_narrow_design() {
    let root = tempfile::tempdir().unwrap();
    let context = PrintContextLayer {
        id: "elevation_contours".into(),
        label: "Elevation contours".into(),
        source: PrintContextSource::ElevationContours {
            buffer_m: 200.0,
            minor_interval_m: 10,
            index_interval_m: 50,
            sample_spacing_m: 10.0,
        },
    };
    let mut provider = Footprints {
        calls: 0,
        fail_first: false,
    };
    let mut fetch = no_fetch;
    let mut hosts = Hosts {
        provider: &mut provider,
        fetch: &mut fetch,
    };
    let print = read_print_context(
        root.path(),
        &scope(),
        "tx",
        &long_narrow_design(),
        &[context],
        &[],
        Mode::Acquire(&mut hosts),
    )
    .unwrap();
    let document: Value = serde_json::from_slice(
        print
            .document
            .as_deref()
            .unwrap_or_else(|| panic!("missing contour context: {print:?}")),
    )
    .unwrap();
    let ring = document["coverage"]["coordinates"][0][0]
        .as_array()
        .unwrap();
    let south = ring[0][1].as_f64().unwrap();
    let north = ring[2][1].as_f64().unwrap();
    assert!(south < -2.01 && north > -1.99, "{ring:?}");
    assert!(print.layers.contains(&"contours_index".to_string()));
}

#[test]
fn a_desktop_held_source_is_refused_not_omitted() {
    let root = tempfile::tempdir().unwrap();
    let contexts = vec![PrintContextLayer {
        id: "survey_1".into(),
        label: "Survey".into(),
        source: PrintContextSource::Survey {
            form: "customer_survey".into(),
        },
    }];
    let refused = read_print_context(
        root.path(),
        &scope(),
        "tx",
        &design(),
        &contexts,
        &[],
        Mode::Read,
    )
    .expect_err("refused");
    assert_eq!(refused.code(), "print_context_unsupported");
}

/// Cloud-resident rows (foundation-datasets contract R1/R2/R4/R6): valid only
/// without a bundle, never declared, never a seed target, named by id, layer
/// or alias, and refused by name when a seed asks for one.
#[test]
fn a_cloud_resident_row_seeds_through_its_bounded_read() {
    let roads = ReferenceResource {
        id: "a".repeat(64),
        version: "b".repeat(64),
        label: "Roads".into(),
        layer: "roads".into(),
        country: "Rwanda".into(),
        kind: "bigquery".into(),
        active: true,
        download_url: Some("https://storage.googleapis.com/bucket/roads.geojsonl.gz".into()),
        bundle_sha256: Some("c".repeat(64)),
        expanded_sha256: Some("d".repeat(64)),
        download_bytes: Some(10),
        expanded_bytes: Some(20),
        bundle_features: Some(3),
        bundle_built_at: Some(1),
        ..Default::default()
    };
    let parcels = ReferenceResource {
        id: "e".repeat(64),
        layer: "rwanda_upi_parcels".into(),
        label: "Land parcels (UPI)".into(),
        residency: "cloud".into(),
        residency_reason: Some("declared foundation dataset".into()),
        download_url: None,
        bundle_sha256: None,
        expanded_sha256: None,
        download_bytes: None,
        expanded_bytes: None,
        bundle_features: None,
        bundle_built_at: None,
        ..roads.clone()
    };
    let villages = ReferenceResource {
        id: "1".repeat(64),
        layer: "village_boundaries".into(),
        label: "Village boundaries".into(),
        aliases: vec!["rwanda_villages".into(), "2".repeat(64)],
        ..roads.clone()
    };
    parcels
        .validate()
        .expect("a cloud row without a bundle is valid");
    assert!(
        ReferenceResource {
            download_url: Some("https://storage.googleapis.com/bucket/p.geojsonl.gz".into()),
            ..parcels.clone()
        }
        .validate()
        .is_err(),
        "a cloud row publishing a bundle is a contradiction"
    );
    assert!(ReferenceResource {
        residency: "orbit".into(),
        ..parcels.clone()
    }
    .validate()
    .is_err());
    let rows = [roads.clone(), parcels.clone(), villages.clone()];
    let declared = declared(&rows).expect("declaration");
    assert!(
        !declared.iter().any(|d| d.dataset.id == parcels.id),
        "a cloud row is never declared"
    );
    // Named by id, layer and alias; the answered-as note travels.
    let by_layer = ds_project_data::named(&rows, "rwanda_villages").unwrap();
    assert_eq!(by_layer.resource.id, villages.id);
    assert_eq!(by_layer.answered_as.as_deref(), Some("rwanda_villages"));
    let by_alias_id = ds_project_data::named(&rows, &"2".repeat(64)).unwrap();
    assert_eq!(by_alias_id.resource.id, villages.id);
    assert!(ds_project_data::named(&rows, &villages.id)
        .unwrap()
        .answered_as
        .is_none());
    assert!(ds_project_data::named(&rows, "nothing").is_none());
    // A seed naming a cloud row — by id or layer — resolves to the bounded
    // read as its provider: the project's cells are read, never a bundle.
    for needle in [parcels.id.as_str(), "rwanda_upi_parcels"] {
        let resolved = ds_project_data::declared::resolve(&rows, needle)
            .expect("a cloud row is a seed target")
            .expect("named");
        assert_eq!(resolved.dataset.id, parcels.id);
        assert_eq!(
            resolved.dataset.provider,
            ds_project_data::declared::CLOUD_READ_PROVIDER
        );
        assert_eq!(resolved.dataset.parameters["layer"], "rwanda_upi_parcels");
        assert!(matches!(resolved.source, DeclaredSource::Cloud(_)));
    }
    // A retired broad layer names its detailed alternatives.
    match ds_project_data::declared::resolve(&rows, "powerlines") {
        Err(Failure::Retired(message)) => assert!(message.contains("hv_line, mv_line, lv_line")),
        other => panic!("retired layer resolved: {other:?}"),
    }
    assert_eq!(Failure::Retired(String::new()).code(), "dataset_retired");
    // An alias resolves to its authority as a seed target.
    let resolved = ds_project_data::declared::resolve(&rows, "rwanda_villages")
        .unwrap()
        .unwrap();
    assert_eq!(resolved.dataset.id, villages.id);
}

/// Answers every area with one footprint at its centre, and records how large
/// each area it was asked for was.
struct AreaRecorder {
    areas: Vec<f64>,
}

impl Provider for AreaRecorder {
    fn acquire(&mut self, dataset: &Dataset, area: &Value) -> Result<Acquisition, Failure> {
        let ring = &area["coordinates"][0];
        let (w, s) = (ring[0][0].as_f64().unwrap(), ring[0][1].as_f64().unwrap());
        let (e, n) = (ring[2][0].as_f64().unwrap(), ring[2][1].as_f64().unwrap());
        self.areas
            .push(ds_command_kernel::project_dataset_cache::bounds_area_m2([w, s, e, n]).unwrap());
        Ok(Acquisition {
            features: vec![footprint(self.areas.len(), (w + e) / 2.0, (s + n) / 2.0)],
            source_version: format!("{}-v1", dataset.id),
            source_receipt: json!({"provider": "recorder"}),
        })
    }
}

/// 86d184cf: a whole project's overview is one district-sized gap. It is
/// acquired in tiles no provider read may exceed, never as one request the
/// provider refuses — and a warm seed then asks for nothing.
#[test]
fn a_district_sized_gap_is_acquired_in_tiles_no_provider_read_exceeds() {
    use ds_command_kernel::project_dataset_cache::MAX_ENVELOPE_M2;
    let root = tempfile::tempdir().unwrap();
    let district = json!({"mv_lines": {"type": "FeatureCollection", "features": [
        {"type": "Feature", "properties": {}, "geometry": {"type": "LineString",
            "coordinates": [[29.40, -2.50], [29.70, -2.20]]}}
    ]}});
    let extents = design_extents([("mv_data", &district)]).unwrap();
    let policy = buffer_policy(&[200.0]).unwrap();
    let mut provider = AreaRecorder { areas: Vec::new() };
    let mut fetch = no_fetch;
    let mut seed = |provider: &mut AreaRecorder| {
        let mut hosts = Hosts {
            provider,
            fetch: &mut fetch,
        };
        ensure(
            root.path(),
            &scope(),
            &buildings_dataset(),
            &DeclaredSource::Buildings,
            &policy,
            &extents,
            &[],
            Mode::Acquire(&mut hosts),
        )
        .expect("a district seeds in tiles")
    };
    let first = seed(&mut provider);
    assert!(first.covered, "{:?}", first.warnings);
    assert!(provider.areas.len() > 1, "one district-sized read was made");
    assert_eq!(first.acquired, provider.areas.len());
    for area in &provider.areas {
        assert!(
            *area <= MAX_ENVELOPE_M2,
            "a {area} m² read exceeds the bound"
        );
    }
    let calls = provider.areas.len();
    let second = seed(&mut provider);
    assert!(second.covered);
    assert_eq!(second.acquired, 0);
    assert_eq!(provider.areas.len(), calls, "a warm seed asks for nothing");
}

/// Seed one national layer holding `features` from its installed bundle.
fn seed_national(
    root: &Path,
    features: &[Value],
    design: &Value,
) -> ds_project_data::EnsureOutcome {
    let (compressed, receipt) = bundle(features);
    let resource = roads_resource(&receipt);
    let mut fetch = move |_: &BundleReceipt, dest: &Path| -> Result<(), String> {
        std::fs::write(dest, &compressed).map_err(|e| e.to_string())
    };
    let mut provider = Footprints {
        calls: 0,
        fail_first: false,
    };
    let extents = design_extents([("tx", design)]).unwrap();
    let policy = buffer_policy(&[200.0]).unwrap();
    let outcome = {
        let mut hosts = Hosts {
            provider: &mut provider,
            fetch: &mut fetch,
        };
        ensure(
            root,
            &scope(),
            &catalog_dataset(&resource),
            &DeclaredSource::Catalog(Box::new(resource.clone())),
            &policy,
            &extents,
            &[],
            Mode::Acquire(&mut hosts),
        )
        .expect("a national subset seeds")
    };
    assert_eq!(
        provider.calls, 0,
        "a national subset never asks the provider"
    );
    outcome
}

fn point(id: String, x: f64, y: f64) -> Value {
    json!({"type": "Feature", "properties": {"id": id},
        "geometry": {"type": "Point", "coordinates": [x, y]}})
}

/// 86d184cf: a national-seed subset denser than one index read returns is
/// split into quarters and held whole, not refused for the whole layer.
/// 2026-09-27 review: on the round grid the quarters' replacements dropped
/// 100 of these 10,050 rows while the seed read covered.
#[test]
fn a_truncated_national_read_is_split_not_refused() {
    let dense = json!({"lv_poles": {"type": "FeatureCollection", "features": [
        {"type": "Feature", "properties": {}, "geometry": {"type": "Point", "coordinates": [30.001, -1.999]}}
    ]}});
    for (grid, offset) in [("round", 0.0), ("offset", 0.0000071)] {
        let root = tempfile::tempdir().unwrap();
        let points: Vec<Value> = (0..10_050)
            .map(|index| {
                point(
                    format!("p{index}"),
                    30.0 + offset + (index % 101) as f64 * 0.00002,
                    -2.0 - offset + (index / 101) as f64 * 0.00002,
                )
            })
            .collect();
        let outcome = seed_national(root.path(), &points, &dense);
        assert!(outcome.covered, "{grid}: {:?}", outcome.warnings);
        assert!(outcome.acquired > 1, "{grid}: the dense cell was split");
        assert_eq!(outcome.features, 10_050, "{grid} grid");
    }
}

/// Records the bounds of every tile it is asked for, and answers nothing.
struct TileRecorder {
    tiles: Vec<[f64; 4]>,
}

impl Provider for TileRecorder {
    fn acquire(&mut self, dataset: &Dataset, area: &Value) -> Result<Acquisition, Failure> {
        let ring = &area["coordinates"][0];
        let at = |corner: usize, axis: usize| ring[corner][axis].as_f64().unwrap();
        self.tiles.push([at(0, 0), at(0, 1), at(2, 0), at(2, 1)]);
        Ok(Acquisition {
            features: Vec::new(),
            source_version: format!("{}-v1", dataset.id),
            source_receipt: json!({"provider": "tile-recorder"}),
        })
    }
}

/// 2026-09-27 review: a tiled national seed holds every row its tiles share.
/// The room replaces a tile's rows under the kernel's selection rule; a page
/// read under a narrower one lost what a later tile's replacement removed —
/// points a float's width from an interior edge, and every road whose bounds
/// cross into a later tile it never enters — while the seed read covered.
#[test]
fn a_tiled_national_seed_keeps_every_row_its_tiles_share() {
    let district = json!({"mv_lines": {"type": "FeatureCollection", "features": [
        {"type": "Feature", "properties": {}, "geometry": {"type": "LineString",
            "coordinates": [[29.40, -2.50], [29.70, -2.20]]}}
    ]}});
    // The interior edges of the tiles a seed of this plan reads.
    let scratch = tempfile::tempdir().unwrap();
    let mut recorder = TileRecorder { tiles: Vec::new() };
    let mut fetch = no_fetch;
    let mut hosts = Hosts {
        provider: &mut recorder,
        fetch: &mut fetch,
    };
    ensure(
        scratch.path(),
        &scope(),
        &buildings_dataset(),
        &DeclaredSource::Buildings,
        &buffer_policy(&[200.0]).unwrap(),
        &design_extents([("tx", &district)]).unwrap(),
        &[],
        Mode::Acquire(&mut hosts),
    )
    .unwrap();
    let tiles = recorder.tiles;
    assert!(tiles.len() >= 9, "{} tiles", tiles.len());
    let edges = |axis: usize| {
        let outer = tiles.iter().map(|t| t[axis + 2]).fold(f64::MIN, f64::max);
        let mut edges: Vec<f64> = tiles
            .iter()
            .map(|t| t[axis + 2])
            .filter(|edge| *edge < outer)
            .collect();
        edges.sort_by(f64::total_cmp);
        edges.dedup();
        edges
    };
    let (columns, rows) = (edges(0), edges(1));
    assert!(columns.len() >= 2 && rows.len() >= 2);
    let (x0, y0) = (
        (tiles[0][0] + tiles[0][2]) / 2.0,
        (tiles[0][1] + tiles[0][3]) / 2.0,
    );
    // Round: on each edge. Offset: a float's width to a metre either side.
    let offsets = [
        -1e-5, -2e-6, -1e-6, -3e-7, -1e-7, 0.0, 1e-7, 3e-7, 1e-6, 2e-6, 1e-5,
    ];
    let mut rows_held = Vec::new();
    for (i, x) in columns.iter().enumerate() {
        for (j, d) in offsets.iter().enumerate() {
            rows_held.push(point(format!("v{i}-{j}"), x + d, y0));
        }
    }
    for (i, y) in rows.iter().enumerate() {
        for (j, d) in offsets.iter().enumerate() {
            rows_held.push(point(format!("h{i}-{j}"), x0, y + d));
        }
    }
    // North-west to south-east across several tiles, an L down the west and
    // along the south, and a triangle leaning the same way: each one's bounds
    // reach a north-eastern tile it never enters.
    for k in 0..5 {
        let x = 29.42 + k as f64 * 0.01;
        rows_held.push(
            json!({"type": "Feature", "properties": {"id": format!("d{k}")},
            "geometry": {"type": "LineString", "coordinates": [[x, -2.22], [x + 0.15, -2.37]]}}),
        );
    }
    rows_held.push(json!({"type": "Feature", "properties": {"id": "l"},
        "geometry": {"type": "LineString", "coordinates": [[29.45, -2.30], [29.45, -2.45], [29.60, -2.45]]}}));
    rows_held.push(json!({"type": "Feature", "properties": {"id": "t"},
        "geometry": {"type": "Polygon", "coordinates": [[[29.47, -2.27], [29.47, -2.42], [29.62, -2.42], [29.47, -2.27]]]}}));
    let root = tempfile::tempdir().unwrap();
    let outcome = seed_national(root.path(), &rows_held, &district);
    assert!(outcome.covered, "{:?}", outcome.warnings);
    assert_eq!(outcome.acquired, tiles.len(), "one commit per tile");
    assert_eq!(outcome.features, rows_held.len() as i64);
}

/// 2026-09-27 review: a later seed never loses a row an earlier one holds. The
/// road is held from the first seed's tile; its bounds reach across the
/// interior edge into the tile the grown design adds, where the road never
/// goes. That tile's replacement selected the road by its bounds grown by a
/// provider margin and deleted it, and the index, answering exact
/// intersection, never handed it back.
#[test]
fn an_incremental_national_seed_keeps_what_an_earlier_seed_holds() {
    let poles = |points: &[[f64; 2]]| {
        let features: Vec<Value> = points
            .iter()
            .map(|p| json!({"type": "Feature", "properties": {}, "geometry": {"type": "Point", "coordinates": p}}))
            .collect();
        json!({"lv_poles": {"type": "FeatureCollection", "features": features}})
    };
    let first = poles(&[[30.0, -2.0]]);
    let grown = poles(&[[30.0, -2.0], [30.012, -2.0]]);
    // The first seed's tile, then the one tile the grown design adds.
    let scratch = tempfile::tempdir().unwrap();
    let mut recorded = Vec::new();
    for design in [&first, &grown] {
        let mut recorder = TileRecorder { tiles: Vec::new() };
        let mut fetch = no_fetch;
        let mut hosts = Hosts {
            provider: &mut recorder,
            fetch: &mut fetch,
        };
        ensure(
            scratch.path(),
            &scope(),
            &buildings_dataset(),
            &DeclaredSource::Buildings,
            &buffer_policy(&[200.0]).unwrap(),
            &design_extents([("tx", design)]).unwrap(),
            &[],
            Mode::Acquire(&mut hosts),
        )
        .unwrap();
        recorded.push(recorder.tiles);
    }
    let ([held], [added]) = (recorded[0].as_slice(), recorded[1].as_slice()) else {
        panic!("one tile each: {recorded:?}");
    };
    let [_, s, e, n] = *held;
    let [west, south, _, north] = *added;
    assert_eq!(west, e, "the added tile shares the held tile's east edge");
    assert!(s < south - 0.0045 && north < n, "{added:?}");
    let mid = (south + north) / 2.0;
    let rows = vec![
        point("inside".into(), e - 0.001, mid),
        point("added".into(), e + 0.001, mid),
        // On the interior edge, and half a metre inside the held tile.
        point("edge".into(), e, mid),
        point("near".into(), e - 0.000005, mid),
        // Leaves the held tile through that edge 200 m below the added tile,
        // whose south-west corner its bounds cover.
        json!({"type": "Feature", "properties": {"id": "road"}, "geometry": {"type": "LineString",
            "coordinates": [[e - 0.0005, south + 0.0005], [e + 0.0005, south - 0.0045]]}}),
    ];
    let root = tempfile::tempdir().unwrap();
    let before = seed_national(root.path(), &rows, &first);
    assert!(before.covered, "{:?}", before.warnings);
    assert_eq!(before.features, 4, "all but the added tile's row");
    let after = seed_national(root.path(), &rows, &grown);
    assert!(after.covered, "{:?}", after.warnings);
    assert_eq!(after.acquired, 1, "only the added tile is read");
    assert_eq!(
        after.features, 5,
        "the earlier seed's rows are all still held"
    );
}

/// 2026-09-27 review: tiling had no upper bound, so an extent no project has
/// became tens of thousands of sequential provider rounds. 2026-10-23: such
/// an extent (a vertex left at zero) is named and never read, and it no
/// longer refuses the project's real designs: on the Rwanda template one
/// stray design had failed every one of its 42 datasets. A district still
/// seeds (above).
#[test]
fn an_implausibly_wide_extent_is_named_and_never_read_while_real_designs_seed() {
    use ds_command_kernel::project_dataset_cache::MAX_ENVELOPE_M2;
    let root = tempfile::tempdir().unwrap();
    // Nine degrees by eight and a half: about 37,000 tiles of 25 km².
    let wide = json!({"lv_lines": {"type": "FeatureCollection", "features": [
        {"type": "Feature", "properties": {}, "geometry": {"type": "LineString",
            "coordinates": [[29.0, -2.5], [38.0, 6.0]]}}
    ]}});
    let real = json!({"lv_lines": {"type": "FeatureCollection", "features": [
        {"type": "Feature", "properties": {}, "geometry": {"type": "LineString",
            "coordinates": [[29.70, -2.70], [29.702, -2.698]]}}
    ]}});
    let mut provider = AreaRecorder { areas: Vec::new() };
    let mut fetch = no_fetch;
    let mut hosts = Hosts {
        provider: &mut provider,
        fetch: &mut fetch,
    };
    let seeded = ensure(
        root.path(),
        &scope(),
        &buildings_dataset(),
        &DeclaredSource::Buildings,
        &buffer_policy(&[200.0]).unwrap(),
        &design_extents([("tx", &wide), ("tx-real", &real)]).unwrap(),
        &[],
        Mode::Acquire(&mut hosts),
    )
    .expect("the real design seeds beside the stray one");
    assert!(seeded.covered, "{:?}", seeded.warnings);
    assert!(
        seeded
            .warnings
            .iter()
            .any(|warning| warning.contains("correct their geometry: tx")),
        "{:?}",
        seeded.warnings
    );
    assert_eq!(
        provider.areas.len(),
        1,
        "one read, for the real design only"
    );
    assert!(provider.areas[0] <= MAX_ENVELOPE_M2);
}

/// Serves a fixed set of rows: each one the asked area selects under the
/// room's own rule, as the governed provider does. Records every area asked
/// for, and fails the call numbered `fail_at` (1-based).
struct Fixed {
    rows: Vec<Value>,
    areas: Vec<Value>,
    fail_at: Option<usize>,
}

impl Provider for Fixed {
    fn acquire(&mut self, dataset: &Dataset, area: &Value) -> Result<Acquisition, Failure> {
        self.areas.push(area.clone());
        if self.fail_at == Some(self.areas.len()) {
            return Err(Failure::ProviderUnavailable("provider is down".into()));
        }
        let features = self
            .rows
            .iter()
            .filter(|row| selects(dataset, row, area))
            .cloned()
            .collect();
        Ok(Acquisition {
            features,
            source_version: format!("{}-v1", dataset.id),
            source_receipt: json!({"provider": "fixed"}),
        })
    }
}

fn selects(dataset: &Dataset, row: &Value, area: &Value) -> bool {
    ds_command_kernel::project_dataset_cache::acquisition_selects(
        &dataset.id,
        &dataset.provider,
        row,
        area,
    )
    .unwrap()
}

/// A district-sized plan, so a refresh has many tiles to read again.
fn refresh_district() -> Value {
    json!({"mv_lines": {"type": "FeatureCollection", "features": [
        {"type": "Feature", "properties": {}, "geometry": {"type": "LineString",
            "coordinates": [[29.40, -2.50], [29.70, -2.20]]}}
    ]}})
}

/// The provider's rows along the district line, and a pre-fix seed of them:
/// every third row missing and one row the provider never had.
fn truth_and_flawed() -> (Vec<Value>, Vec<Value>) {
    let truth: Vec<Value> = (0..=60)
        .map(|index| {
            let step = index as f64 * 0.3 / 60.0;
            footprint(index, 29.40 + step, -2.50 + step)
        })
        .collect();
    let mut flawed: Vec<Value> = truth
        .iter()
        .enumerate()
        .filter(|(index, _)| index % 3 != 0)
        .map(|(_, row)| row.clone())
        .collect();
    let mut ghost = footprint(0, 29.55, -2.35);
    ghost["properties"]["id"] = json!("ghost");
    flawed.push(ghost);
    (truth, flawed)
}

fn ids(features: &[Value]) -> Vec<String> {
    let mut ids: Vec<String> = features
        .iter()
        .map(|row| row["properties"]["id"].as_str().unwrap().to_owned())
        .collect();
    ids.sort();
    ids
}

/// Seed (`refresh == false`) or refresh the district's buildings.
fn settle_district(
    root: &Path,
    provider: &mut Fixed,
    refresh: bool,
) -> Result<ds_project_data::EnsureOutcome, Failure> {
    let extents = design_extents([("mv_data", &refresh_district())]).unwrap();
    let policy = buffer_policy(&[200.0]).unwrap();
    let mut fetch = no_fetch;
    let mut hosts = Hosts {
        provider,
        fetch: &mut fetch,
    };
    let (dataset, source) = (buildings_dataset(), DeclaredSource::Buildings);
    if refresh {
        ds_project_data::refresh(
            root,
            &scope(),
            &dataset,
            &source,
            &policy,
            &extents,
            &[],
            &mut hosts,
        )
    } else {
        ensure(
            root,
            &scope(),
            &dataset,
            &source,
            &policy,
            &extents,
            &[],
            Mode::Acquire(&mut hosts),
        )
    }
}

/// What the room holds over the district's planned coverage, read only.
fn held_district(root: &Path) -> (ds_project_data::EnsureOutcome, Vec<String>) {
    let extents = design_extents([("mv_data", &refresh_district())]).unwrap();
    let read = ensure(
        root,
        &scope(),
        &buildings_dataset(),
        &DeclaredSource::Buildings,
        &buffer_policy(&[200.0]).unwrap(),
        &extents,
        &[],
        Mode::Read,
    )
    .unwrap();
    let area = read.coverage_geometry.clone().unwrap();
    let (features, _) =
        ds_project_data::ensure::read_features(root, &scope(), &buildings_dataset(), &area)
            .unwrap();
    (read, ids(&features))
}

/// 468826f: rooms seeded before the national tiling fix claim coverage over
/// rows they may miss, and a warm seed reads nothing. The explicit refresh
/// reads every planned tile again and the room then holds exactly the
/// provider's rows; the next ordinary seed is warm again.
#[test]
fn an_explicit_refresh_reacquires_the_providers_exact_rows_and_the_next_seed_stays_warm() {
    let root = tempfile::tempdir().unwrap();
    let (truth, flawed) = truth_and_flawed();
    let mut old = Fixed {
        rows: flawed.clone(),
        areas: Vec::new(),
        fail_at: None,
    };
    let seeded = settle_district(root.path(), &mut old, false).unwrap();
    assert!(seeded.covered, "{:?}", seeded.warnings);
    let tiles = old.areas.len();
    assert!(tiles >= 9, "{tiles} tiles");
    assert_eq!(held_district(root.path()).1, ids(&flawed));

    // The defect: coverage answers the plan, so an ordinary seed — refresh is
    // never implicit — asks the correct provider for nothing and keeps the
    // flawed rows.
    let mut provider = Fixed {
        rows: truth.clone(),
        areas: Vec::new(),
        fail_at: None,
    };
    let warm = settle_district(root.path(), &mut provider, false).unwrap();
    assert!(warm.covered);
    assert_eq!((warm.acquired, provider.areas.len()), (0, 0));
    assert_eq!(held_district(root.path()).1, ids(&flawed));

    let refreshed = settle_district(root.path(), &mut provider, true).unwrap();
    assert!(refreshed.covered, "{:?}", refreshed.warnings);
    assert_eq!(refreshed.skipped, 0);
    assert_eq!(refreshed.acquired, provider.areas.len());
    assert_eq!(provider.areas, old.areas, "every planned tile, read again");
    assert_eq!(refreshed.features, truth.len() as i64);
    let (read, held) = held_district(root.path());
    assert!(read.covered);
    assert_eq!(held, ids(&truth), "missing rows back, the ghost gone");
    let queries = refreshed.status["queries"].as_array().unwrap();
    assert!(
        queries.iter().any(|row| row["refresh"] == true),
        "the room records the refresh as one"
    );

    let calls = provider.areas.len();
    let after = settle_district(root.path(), &mut provider, false).unwrap();
    assert!(after.covered);
    assert_eq!(after.acquired, 0);
    assert_eq!(
        provider.areas.len(),
        calls,
        "a seed after a refresh is warm"
    );
    assert_eq!(held_district(root.path()).1, ids(&truth));
}

/// A refresh that fails part-way costs nothing held: tiles it committed hold
/// the provider's rows, every other tile keeps its old rows, completed
/// coverage still answers a read, and running the refresh again completes it.
#[test]
fn a_refresh_that_fails_part_way_keeps_every_completed_holding() {
    let root = tempfile::tempdir().unwrap();
    let (truth, flawed) = truth_and_flawed();
    let mut old = Fixed {
        rows: flawed.clone(),
        areas: Vec::new(),
        fail_at: None,
    };
    settle_district(root.path(), &mut old, false).unwrap();
    let before = held_district(root.path()).0.status;

    let mut failing = Fixed {
        rows: truth.clone(),
        areas: Vec::new(),
        fail_at: Some(3),
    };
    let refused = settle_district(root.path(), &mut failing, true).unwrap_err();
    assert_eq!(refused.code(), "dataset_provider_unavailable");
    assert_eq!(failing.areas.len(), 3, "the refresh stopped at the failure");

    let dataset = buildings_dataset();
    let refreshed = &failing.areas[..2];
    let in_refreshed = |row: &&Value| refreshed.iter().any(|a| selects(&dataset, row, a));
    let mut expected: Vec<Value> = truth.iter().filter(in_refreshed).cloned().collect();
    expected.extend(flawed.iter().filter(|row| !in_refreshed(row)).cloned());
    let (read, held) = held_district(root.path());
    assert!(read.covered, "completed coverage survives a failed refresh");
    assert_eq!(read.status["completed"], before["completed"]);
    assert_eq!(held, ids(&expected));
    assert_ne!(held, ids(&flawed), "the committed tiles were refreshed");

    let mut provider = Fixed {
        rows: truth.clone(),
        areas: Vec::new(),
        fail_at: None,
    };
    let repeated = settle_district(root.path(), &mut provider, true).unwrap();
    assert!(repeated.covered, "{:?}", repeated.warnings);
    assert_eq!(held_district(root.path()).1, ids(&truth));
    assert_eq!(provider.areas, old.areas);
}
