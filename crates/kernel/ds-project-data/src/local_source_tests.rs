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
    let converted = ds_columnar::layers_to_geoparquet(
        "fixture.geojson",
        &bytes,
        None,
        vec![ds_columnar::SourceLayer {
            name: "fixture".into(),
            features,
            style_document: None,
        }],
        None,
        0,
    )
    .unwrap();
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
