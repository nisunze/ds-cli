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
        .any(|o| o.layer == "roads" && o.reason == "project_dataset_not_held"));
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
    assert_eq!(document["coverage"]["type"], "Polygon");
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
    // Installing it nationally is still refused by name.
    assert!(
        ds_project_data::declared::cloud_only_message(&parcels).contains("project-cache seed"),
        "the refusal points at the per-project seed"
    );
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
