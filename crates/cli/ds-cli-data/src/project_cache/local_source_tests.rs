use super::*;
use ds_project_data::local_source::{DigestAlgorithm, Kind, Origin, ProducerSource};
use sha2::{Digest, Sha256};
use std::{
    cell::Cell,
    time::{SystemTime, UNIX_EPOCH},
};

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "ds-cli-local-source-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn scope() -> Scope {
    Scope {
        principal: "fixture-user".into(),
        project: "fixture-project".into(),
    }
}
fn input(command: &'static Command, tokens: &[&str]) -> Inputs {
    ds_cli_contract::args::parse(
        command,
        &tokens
            .iter()
            .map(|token| token.to_string())
            .collect::<Vec<_>>(),
    )
    .unwrap()
}
fn fixture(dir: &Path) -> (Source, PathBuf) {
    let footprint = |id, x: f64| {
        json!({"type":"Feature","id":id,"properties":{"area_in_meters":40},
        "geometry":{"type":"Polygon","coordinates":[[[x,-2.],[x+0.0001,-2.],
            [x+0.0001,-1.9999],[x,-1.9999],[x,-2.]]]}})
    };
    let first = footprint("building-a", 30.0005);
    let bytes = serde_json::to_vec(&json!({"type":"FeatureCollection","features":[
        first.clone(), first, footprint("building-b",30.0007), footprint("building-c",30.0605)]}))
    .unwrap();
    let converted =
        ds_columnar::source_to_geoparquet("fixture.geojson", &bytes, None, None).unwrap();
    let artifact = dir.join("input.parquet");
    std::fs::write(&artifact, converted.parquet).unwrap();
    let source = Source {
        schema: "ds.local-analytical-source/v1".into(),
        project_id: scope().project,
        asset_id: "custom-footprints".into(),
        revision: "revision-1".into(),
        actual_source: Origin::Custom,
        producer_source: ProducerSource {
            revision: "producer-1".into(),
            digest_algorithm: DigestAlgorithm::Sha256,
            digest: format!("{:x}", Sha256::digest(&bytes)),
        },
        content: Kind::Footprints,
        artifact_sha256: ds_columnar::digest_file(&artifact).unwrap(),
        artifact_path: artifact,
        completed_coverage: policy::Coverage {
            cells: vec![[29.9, -2.1, 30.2, -1.9]],
        },
    };
    let manifest = dir.join("source.json");
    std::fs::write(&manifest, serde_json::to_vec(&source).unwrap()).unwrap();
    (source, manifest)
}
fn extents(expanded: bool) -> Vec<policy::Extent> {
    let mut extents = vec![policy::Extent {
        name: "transformer-a".into(),
        bounds: Some([30., -2., 30.001, -1.999]),
    }];
    if expanded {
        extents.push(policy::Extent {
            name: "transformer-b".into(),
            bounds: Some([30.06, -2., 30.061, -1.999]),
        });
    }
    extents
}
fn declared(inputs: &Inputs, scope: &Scope, catalogue_reads: &Cell<usize>) -> Declared {
    dispatch(inputs, scope, Ok, || {
        catalogue_reads.set(catalogue_reads.get() + 1);
        Err(Failure::failed(
            "catalog_unavailable",
            "no catalogue is available offline",
        ))
    })
    .unwrap()
}

fn publisher(dir: &Path, origin: Origin) -> (PathBuf, PathBuf) {
    use ds_project_data::publisher_footprints::{Input, Resource};
    use std::io::Write;
    let gzip = dir.join("publisher.gz");
    let raw = if origin == Origin::GoogleOpenBuildings {
        "geometry,building_id,confidence\n\"POLYGON ((30.0005 -2,30.0006 -2,30.0006 -1.9999,30.0005 -1.9999,30.0005 -2))\",source-id,0.9\n".into()
    } else {
        json!({"type":"Feature","id":"source-id","properties":{"height":9.},
            "geometry":{"type":"Polygon","coordinates":[[[30.0005,-2.],[30.0006,-2.],[30.0006,-1.9999],[30.0005,-1.9999],[30.0005,-2.]]]}}).to_string()+"\n"
    };
    let mut writer = flate2::write::GzEncoder::new(
        std::fs::File::create(&gzip).unwrap(),
        flate2::Compression::default(),
    );
    writer.write_all(raw.as_bytes()).unwrap();
    writer.finish().unwrap();
    let coverage = policy::Coverage {
        cells: vec![[29.9, -2.1, 30.2, -1.9]],
    };
    let source = Input {
        schema: "ds.publisher-footprints/v1".into(),
        project_id: scope().project,
        asset_id: "publisher-city".into(),
        revision: "asset-r1".into(),
        actual_source: origin,
        publisher_revision: "exact-publisher-version".into(),
        coverage: coverage.clone(),
        resources: vec![Resource {
            resource_id: "publisher-tile".into(),
            sha256: ds_columnar::digest_file(&gzip).unwrap(),
            path: gzip.clone(),
            coverage,
        }],
    };
    let manifest = dir.join("publisher.json");
    std::fs::write(&manifest, serde_json::to_vec(&source).unwrap()).unwrap();
    (manifest, gzip)
}

#[test]
fn publisher_selector_materializes_actual_local_sources_and_consumes_them_offline() {
    for origin in [Origin::GoogleOpenBuildings, Origin::MicrosoftBuildings] {
        let dir = Fixture::new();
        let root = dir.0.join("held");
        let (manifest, gzip) = publisher(&dir.0, origin);
        let inputs = input(
            &SEED_COMMAND,
            &[
                "--project",
                "fixture-project",
                "--publisher-input",
                manifest.to_str().unwrap(),
            ],
        );
        let selected = select_seed(&inputs, &scope()).unwrap().unwrap();
        let mut engine = Engine::default();
        let (entry, produced) =
            materialize(&root, &scope(), selected, &mut |r| engine.convert(r)).unwrap();
        assert_eq!(engine.calls, 1, "the conversion is the engine's");
        assert_eq!(produced["actual_source"], json!(origin));
        assert_eq!(produced["reused"], false);
        counts::take();
        let first = seed(&root, &scope(), &entry, &extents(false), false, || Ok(())).unwrap();
        assert_eq!(first["covered"], true);
        assert_eq!(first["feature_count"], 1);
        std::fs::remove_file(gzip).unwrap();
        let (retained, second) = materialize(
            &root,
            &scope(),
            select_seed(&inputs, &scope()).unwrap().unwrap(),
            &mut |r| engine.convert(r),
        )
        .unwrap();
        assert_eq!(second["reused"], true);
        assert_eq!(engine.calls, 1, "a retained conversion never reaches the engine");
        assert_eq!(
            seed(
                &root,
                &scope(),
                &retained,
                &extents(false),
                false,
                || Ok(())
            )
            .unwrap()["acquired"],
            0
        );
        let source_manifest = second["source_manifest"].as_str().unwrap();
        let status_input = input(
            &STATUS_COMMAND,
            &[
                "--project",
                "fixture-project",
                "--local-source",
                source_manifest,
            ],
        );
        let catalogue_reads = Cell::new(0);
        let held = declared(&status_input, &scope(), &catalogue_reads);
        assert_eq!(
            status(&root, &scope(), "stable", &held, false, || Ok(())).unwrap()["datasets"][0]
                ["ready"],
            true
        );
        let out = dir.0.join("publisher-export.geojson");
        let page=super::super::query_dataset(&root,&scope(),&held.dataset,
            &json!({"type":"Polygon","coordinates":[[[30.,-2.],[30.001,-2.],[30.001,-1.999],[30.,-1.999],[30.,-2.]]]}),
            &out,(10,None,None),||Ok(())).unwrap();
        assert_eq!(page["rows_returned"], 1);
        let exported: Value = serde_json::from_slice(&std::fs::read(out).unwrap()).unwrap();
        assert!(
            exported["features"][0]["id"]
                .as_str()
                .unwrap()
                .ends_with(":source-id")
        );
        assert_eq!(
            (catalogue_reads.get(), counts::take()),
            (0, (0, 0)),
            "offline publisher-source status/query do no catalogue/provider/bundle work"
        );
    }
}

#[test]
fn publisher_selection_refuses_ambiguous_or_foreign_input_before_materialization() {
    let dir = Fixture::new();
    let (manifest, _) = publisher(&dir.0, Origin::MicrosoftBuildings);
    for args in [
        vec![
            "--project",
            "fixture-project",
            "--publisher-input",
            manifest.to_str().unwrap(),
            "--dataset",
            "google_open_buildings",
        ],
        vec![
            "--project",
            "fixture-project",
            "--publisher-input",
            manifest.to_str().unwrap(),
            "--local-source",
            "unread.json",
        ],
    ] {
        assert_eq!(
            select_seed(&input(&SEED_COMMAND, &args), &scope())
                .unwrap_err()
                .code(),
            "publisher_input_invalid"
        );
    }
    let mut foreign = scope();
    foreign.project = "other-project".into();
    assert_eq!(
        select_seed(
            &input(
                &SEED_COMMAND,
                &[
                    "--project",
                    "other-project",
                    "--publisher-input",
                    manifest.to_str().unwrap()
                ]
            ),
            &foreign
        )
        .unwrap_err()
        .code(),
        "publisher_input_invalid"
    );
    for error in [
        ds_project_data::publisher_footprints::Error::Invalid("x".into()),
        ds_project_data::publisher_footprints::Error::Required("x".into()),
        ds_project_data::publisher_footprints::Error::Malformed("x".into()),
        ds_project_data::publisher_footprints::Error::Unsupported("x".into()),
        ds_project_data::publisher_footprints::Error::TooLarge("x".into()),
        ds_project_data::publisher_footprints::Error::Store("x".into()),
    ] {
        let refusal = publisher_refused(error.clone());
        assert_eq!(refusal.code(), error.code());
        assert!(SEED_REFUSALS.iter().any(|r| r.code == refusal.code()));
    }
}

#[test]
fn local_resource_refusal_preserves_legacy_warm_holdings_and_completed_coverage() {
    struct Existing;
    impl Provider for Existing {
        fn acquire(
            &mut self,
            _: &Dataset,
            _: &Value,
        ) -> Result<Acquisition, ds_project_data::Failure> {
            Ok(Acquisition {
                features: vec![],
                source_version: "held-google-version".into(),
                source_receipt: json!({"fixture":"existing completed extract"}),
            })
        }
    }
    let dir = Fixture::new();
    let root = dir.0.join("held");
    let dataset = ds_project_data::buildings_dataset();
    let selected = DeclaredSource::Buildings;
    let policy = buffer_policy(&[200.]).unwrap();
    let mut old = Existing;
    let mut fetch = |_: &BundleReceipt, _: &Path| Err("download forbidden".into());
    let first = ds_project_data::ensure(
        &root,
        &scope(),
        &dataset,
        &selected,
        &policy,
        &extents(false),
        &[],
        Mode::Acquire(&mut Hosts {
            provider: &mut old,
            fetch: &mut fetch,
        }),
    )
    .unwrap();
    assert!(first.covered);
    let status = || {
        ds_layer_store::project_dataset_cache::execute_at(
            &root,
            json!({"action":"status","dataset_key":policy::dataset_key(&scope(),&dataset),
        "principal":scope().principal,"project":scope().project}),
        )
        .unwrap()
    };
    let completed = status()["completed"].clone();
    let mut default = CliProvider {
        lane: "unconfigured-native-lane",
        project: "fixture-project",
    };
    let mut hosts = Hosts {
        provider: &mut default,
        fetch: &mut fetch,
    };
    let warm = ds_project_data::ensure(
        &root,
        &scope(),
        &dataset,
        &selected,
        &policy,
        &extents(false),
        &[],
        Mode::Acquire(&mut hosts),
    )
    .unwrap();
    assert!(warm.covered);
    assert_eq!(
        warm.acquired, 0,
        "warm legacy rooms need no new provider input"
    );
    let error = ds_project_data::ensure(
        &root,
        &scope(),
        &dataset,
        &selected,
        &policy,
        &extents(true),
        &[],
        Mode::Acquire(&mut hosts),
    )
    .unwrap_err();
    assert_eq!(error.code(), "project_dataset_local_source_required");
    assert_eq!(
        status()["completed"],
        completed,
        "missing local expansion input does not erase completed coverage"
    );
    assert!(
        ds_project_data::ensure(
            &root,
            &scope(),
            &dataset,
            &selected,
            &policy,
            &extents(false),
            &[],
            Mode::Read
        )
        .unwrap()
        .covered
    );
}

#[test]
fn local_seed_status_query_dispatch_uses_exact_native_holdings_without_remote_reads() {
    let dir = Fixture::new();
    let root = dir.0.join("held");
    let (source, manifest) = fixture(&dir.0);
    let inputs = input(
        &SEED_COMMAND,
        &[
            "--project",
            "fixture-project",
            "--local-source",
            manifest.to_str().unwrap(),
        ],
    );
    let catalogue_reads = Cell::new(0);
    let scope_checks = Cell::new(0);
    let guard = || {
        scope_checks.set(scope_checks.get() + 1);
        Ok(())
    };
    counts::take();
    let entry = declared(&inputs, &scope(), &catalogue_reads);
    let cold = seed(&root, &scope(), &entry, &extents(false), false, guard).unwrap();
    assert!(cold["covered"] == true && cold["acquired"].as_u64().unwrap() > 0);
    assert_eq!(
        cold["feature_count"], 2,
        "the actual native index deduplicates stable rows"
    );
    std::fs::remove_file(&source.artifact_path).unwrap();
    let warm = seed(&root, &scope(), &entry, &extents(false), false, guard).unwrap();
    assert_eq!(
        warm["acquired"], 0,
        "a warm seed never needs the original artifact"
    );
    let expansion = seed(&root, &scope(), &entry, &extents(true), false, guard).unwrap();
    assert!(expansion["covered"] == true && expansion["acquired"].as_u64().unwrap() > 0);
    assert_eq!(expansion["feature_count"], 3);
    let refresh = seed(&root, &scope(), &entry, &extents(true), true, guard).unwrap();
    assert!(refresh["covered"] == true && refresh["acquired"].as_u64().unwrap() > 0);
    assert_eq!(refresh["feature_count"], 3);
    ds_layer_store::reference_cache::execute_at(
        &root,
        json!({"action":"remove","identity":source.identity()}),
    )
    .unwrap();
    let status_inputs = input(
        &STATUS_COMMAND,
        &[
            "--project",
            "fixture-project",
            "--local-source",
            manifest.to_str().unwrap(),
        ],
    );
    let selected = declared(&status_inputs, &scope(), &catalogue_reads);
    let held = status(&root, &scope(), "stable", &selected, false, guard).unwrap();
    assert_eq!(held["datasets"][0]["ready"], true);
    assert_eq!(held["datasets"][0]["feature_count"], 3);
    assert_eq!(held["catalog"]["read"], false);
    assert_eq!(held["selected_source"]["actual_source"], "custom");
    assert_eq!(held["selected_source"]["revision"], "revision-1");
    assert_eq!(
        held["selected_source"]["producer_source"]["revision"],
        "producer-1"
    );
    let id = source.dataset().id;
    let query_inputs = input(
        &QUERY_COMMAND,
        &[
            "--project",
            "fixture-project",
            "--dataset",
            &id,
            "--local-source",
            manifest.to_str().unwrap(),
            "--boundary",
            "fixture.geojson",
            "--geometry-out",
            "fixture-out.geojson",
        ],
    );
    let selected = declared(&query_inputs, &scope(), &catalogue_reads);
    let unheld_gap = json!({"type":"Polygon","coordinates":[[[30.,-2.],[30.062,-2.],
        [30.062,-1.998],[30.,-1.998],[30.,-2.]]]});
    let refused_path = dir.0.join("unheld-gap.geojson");
    let refusal = super::super::query_dataset(
        &root,
        &scope(),
        &selected.dataset,
        &unheld_gap,
        &refused_path,
        (10, None, None),
        || Ok(()),
    )
    .unwrap_err();
    assert_eq!(refusal.code(), "project_dataset_not_held");
    assert!(!refused_path.exists());
    let boundary = json!({"type":"Polygon","coordinates":[[[30.,-2.],[30.001,-2.],
        [30.001,-1.999],[30.,-1.999],[30.,-2.]]]});
    let first_path = dir.0.join("page-1.geojson");
    let first = super::super::query_dataset(
        &root,
        &scope(),
        &selected.dataset,
        &boundary,
        &first_path,
        (1, None, None),
        guard,
    )
    .unwrap();
    assert_eq!(
        (first["rows_returned"].clone(), first["truncated"].clone()),
        (json!(1), json!(true))
    );
    let second_path = dir.0.join("page-2.geojson");
    let second = super::super::query_dataset(
        &root,
        &scope(),
        &selected.dataset,
        &boundary,
        &second_path,
        (
            1,
            first["next"]["cursor"].as_i64(),
            first["next"]["generation"].as_i64(),
        ),
        guard,
    )
    .unwrap();
    assert_eq!(
        (second["rows_returned"].clone(), second["truncated"].clone()),
        (json!(1), json!(false))
    );
    let first_bytes: Value =
        serde_json::from_slice(&std::fs::read(first_path).unwrap()).unwrap();
    let second_bytes: Value =
        serde_json::from_slice(&std::fs::read(second_path).unwrap()).unwrap();
    assert_eq!(first_bytes["features"][0]["id"], "building-a");
    assert_eq!(second_bytes["features"][0]["id"], "building-b");
    assert_eq!(catalogue_reads.get(), 0);
    assert_eq!(
        counts::take(),
        (0, 0),
        "the actual local adapter ports never acquire or download"
    );
    assert_eq!(
        scope_checks.get(),
        14,
        "all seven native operations guard their captured scope twice"
    );
}

#[test]
fn changed_manifest_revision_and_other_principals_never_reuse_a_selected_room() {
    let dir = Fixture::new();
    let root = dir.0.join("held");
    let (mut source, manifest) = fixture(&dir.0);
    let inputs = input(
        &SEED_COMMAND,
        &[
            "--project",
            "fixture-project",
            "--local-source",
            manifest.to_str().unwrap(),
        ],
    );
    let reads = Cell::new(0);
    let entry = declared(&inputs, &scope(), &reads);
    seed(&root, &scope(), &entry, &extents(false), false, || Ok(())).unwrap();
    source.revision = "revision-2".into();
    std::fs::write(&manifest, serde_json::to_vec(&source).unwrap()).unwrap();
    let changed = declared(&inputs, &scope(), &reads);
    let absent = status(&root, &scope(), "stable", &changed, false, || Ok(())).unwrap();
    assert_eq!(absent["datasets"][0]["seeded"], false);
    let another = Scope {
        principal: "other-user".into(),
        project: scope().project,
    };
    let isolated = status(&root, &another, "stable", &entry, false, || Ok(())).unwrap();
    assert_eq!(isolated["datasets"][0]["seeded"], false);
    assert_eq!(reads.get(), 0);
}

#[test]
fn invalid_local_selectors_never_enter_either_data_continuation() {
    let dir = Fixture::new();
    let (mut source, manifest) = fixture(&dir.0);
    let calls = Cell::new(0);
    let invalid = |inputs: &Inputs| {
        let error = dispatch(
            inputs,
            &scope(),
            |_| {
                calls.set(calls.get() + 1);
                Ok(())
            },
            || {
                calls.set(calls.get() + 1);
                Ok(())
            },
        )
        .unwrap_err();
        assert_eq!(error.code(), "project_dataset_scope_invalid");
        assert_eq!(calls.get(), 0);
    };
    invalid(&input(
        &SEED_COMMAND,
        &[
            "--project",
            "fixture-project",
            "--local-source",
            manifest.to_str().unwrap(),
            "--dataset",
            "google_open_buildings",
        ],
    ));
    source.project_id = "other-project".into();
    std::fs::write(&manifest, serde_json::to_vec(&source).unwrap()).unwrap();
    invalid(&input(
        &STATUS_COMMAND,
        &[
            "--project",
            "fixture-project",
            "--local-source",
            manifest.to_str().unwrap(),
        ],
    ));
    std::fs::write(&manifest, vec![b' '; MAX_MANIFEST_BYTES as usize + 1]).unwrap();
    invalid(&input(
        &STATUS_COMMAND,
        &[
            "--project",
            "fixture-project",
            "--local-source",
            manifest.to_str().unwrap(),
        ],
    ));
    let defaults = input(&STATUS_COMMAND, &["--project", "fixture-project"]);
    dispatch(
        &defaults,
        &scope(),
        |_| panic!("no local selector"),
        || {
            calls.set(calls.get() + 1);
            Ok(())
        },
    )
    .unwrap();
    assert_eq!(calls.get(), 1, "an unselected local input does no work");
    for command in [&SEED_COMMAND, &STATUS_COMMAND, &QUERY_COMMAND] {
        assert!(command.args.iter().any(|arg| arg.name == "local-source"));
        assert!(
            command
                .refusals
                .iter()
                .any(|refusal| refusal.code == "native_profile_not_configured")
        );
    }
}

#[test]
fn query_refuses_a_changed_scope_before_publishing_geojson() {
    let dir = Fixture::new();
    let root = dir.0.join("held");
    let (source, manifest) = fixture(&dir.0);
    let inputs = input(
        &SEED_COMMAND,
        &[
            "--project",
            "fixture-project",
            "--local-source",
            manifest.to_str().unwrap(),
        ],
    );
    let entry = declared(&inputs, &scope(), &Cell::new(0));
    seed(&root, &scope(), &entry, &extents(false), false, || Ok(())).unwrap();
    let boundary = json!({"type":"Polygon","coordinates":[[[30.,-2.],[30.001,-2.],
        [30.001,-1.999],[30.,-1.999],[30.,-2.]]]});
    let out = dir.0.join("refused.geojson");
    let checks = Cell::new(0);
    let error = super::super::query_dataset(
        &root,
        &scope(),
        &source.dataset(),
        &boundary,
        &out,
        (10, None, None),
        || {
            checks.set(checks.get() + 1);
            if checks.get() == 2 {
                Err(Failure::conflict(
                    "project_context_changed",
                    "fixture credential changed",
                ))
            } else {
                Ok(())
            }
        },
    )
    .unwrap_err();
    assert_eq!(error.code(), "project_context_changed");
    assert!(!out.exists());
    assert_eq!(checks.get(), 2);
}

/// The Reporter's `prepare-publisher-footprints` contract answered for the
/// two single-row fixtures above; the normalization itself is proved in
/// `ds-network-reporter/src/publisher_footprints_tests.rs`.
#[derive(Default)]
struct Engine {
    calls: usize,
}

impl Engine {
    fn convert(
        &mut self,
        request: &ds_command_kernel::project_dataset_revisions::tasks::FootprintsRequest,
    ) -> Result<
        ds_command_kernel::project_dataset_revisions::tasks::FootprintsResult,
        ds_project_data::publisher_footprints::Error,
    > {
        use ds_command_kernel::project_dataset_revisions::tasks::{
            FOOTPRINT_CONVERSION, FOOTPRINTS_RESULT, FootprintsResult, Page, Publisher,
        };
        self.calls += 1;
        let (prefix, origin) = match request.publisher {
            Publisher::MicrosoftGeojsonl => ("microsoft", "microsoft_buildings"),
            _ => ("google", "google_open_buildings"),
        };
        let row = json!({"type":"Feature","id":format!("{prefix}:source-id"),
            "geometry":{"type":"Polygon","coordinates":[[[30.0005,-2.0],[30.0006,-2.0],[30.0006,-1.9999],[30.0005,-1.9999],[30.0005,-2.0]]]},
            "properties":{"actual_source":origin,"publisher_id":"source-id","publisher_properties":"{}"}})
        .to_string()
            + "\n";
        std::fs::create_dir_all(&request.out_dir).unwrap();
        let pages = request
            .cells
            .iter()
            .enumerate()
            .map(|(index, cell)| {
                let path = request.out_dir.join(format!("cell-{index:05}.geojsonl"));
                std::fs::write(&path, &row).unwrap();
                Page {
                    cell: *cell,
                    path,
                    features: 1,
                    sha256: format!("{:x}", Sha256::digest(row.as_bytes())),
                }
            })
            .collect();
        Ok(FootprintsResult {
            schema: FOOTPRINTS_RESULT.into(),
            conversion: FOOTPRINT_CONVERSION.into(),
            pages,
            raw_rows: 1,
            duplicates: 0,
            objects: json!([]),
        })
    }
}
