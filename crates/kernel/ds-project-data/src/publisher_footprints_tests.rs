//! The producer's orchestration: pins, staging, the engine exchange, the
//! retained source. Row normalization itself is the Reporter's and is proved
//! there (`ds-network-reporter/src/publisher_footprints_tests.rs`); the fake
//! engine here answers the exact typed contract with fixed pages.
use super::*;
use crate::{Acquisition, BundleReceipt, DeclaredSource, Failure, Hosts, Mode, Provider};
use ds_command_kernel::project_dataset_revisions::tasks::Page;

fn scope() -> Scope {
    Scope {
        principal: "fixture-user".into(),
        project: "fixture-project".into(),
    }
}
fn coverage() -> Coverage {
    Coverage {
        cells: vec![[29.9, -2.1, 30.2, -1.9]],
    }
}
fn footprint(id: &str, x: f64) -> Value {
    json!({"type":"Feature","id":id,"geometry":{"type":"Polygon","coordinates":[[[x,-2.],[x+0.0001,-2.],[x+0.0001,-1.9999],[x,-1.9999],[x,-2.]]]},
        "properties":{"actual_source":"microsoft_buildings","publisher_id":id,"publisher_properties":"{}"}})
}
fn gzip(dir: &Path, name: &str, text: &str) -> Resource {
    let path = dir.join(name);
    let file = File::create(&path).unwrap();
    let mut writer = flate2::write::GzEncoder::new(file, flate2::Compression::default());
    writer.write_all(text.as_bytes()).unwrap();
    writer.finish().unwrap();
    Resource {
        resource_id: name.into(),
        sha256: ds_columnar::digest_file(&path).unwrap(),
        path,
        coverage: coverage(),
    }
}
fn input(dir: &Path, source: Origin) -> Input {
    Input {
        schema: "ds.publisher-footprints/v1".into(),
        project_id: scope().project,
        asset_id: "publisher-city".into(),
        revision: "asset-r1".into(),
        actual_source: source,
        publisher_revision: "microsoft-2026-08-13".into(),
        coverage: coverage(),
        resources: vec![gzip(
            dir,
            "tile.gz",
            "publisher bytes the fake engine does not read\n",
        )],
    }
}

/// A fake engine answering the typed contract with fixed rows per cell, and
/// counting how often it was asked.
struct Engine {
    rows: Vec<Vec<Value>>,
    calls: usize,
    seen: Vec<FootprintsRequest>,
}
impl Engine {
    fn new(rows: Vec<Vec<Value>>) -> Self {
        Self {
            rows,
            calls: 0,
            seen: Vec::new(),
        }
    }
    fn convert(&mut self, request: &FootprintsRequest) -> Result<FootprintsResult, Error> {
        self.calls += 1;
        self.seen.push(request.clone());
        for object in &request.objects {
            assert!(object.path.starts_with(request.out_dir.parent().unwrap()));
            assert_eq!(
                ds_columnar::digest_file(&object.path).unwrap(),
                object.sha256
            );
        }
        std::fs::create_dir_all(&request.out_dir).unwrap();
        let mut pages = Vec::new();
        for (index, cell) in request.cells.iter().enumerate() {
            let path = request.out_dir.join(format!("cell-{index:05}.geojsonl"));
            let rows = self.rows.get(index).cloned().unwrap_or_default();
            let text: String = rows.iter().map(|row| format!("{row}\n")).collect();
            std::fs::write(&path, &text).unwrap();
            pages.push(Page {
                cell: *cell,
                path,
                features: rows.len() as u64,
                sha256: sha(text.as_bytes()),
            });
        }
        Ok(FootprintsResult {
            schema: FOOTPRINTS_RESULT.into(),
            conversion: FOOTPRINT_CONVERSION.into(),
            pages,
            raw_rows: 7,
            duplicates: 0,
            objects: json!([]),
        })
    }
}

struct NoRemote(usize);
impl Provider for NoRemote {
    fn acquire(&mut self, _: &policy::Dataset, _: &Value) -> Result<Acquisition, Failure> {
        self.0 += 1;
        Err(Failure::ProviderUnavailable("network forbidden".into()))
    }
}
fn seed(
    root: &Path,
    source: &Source,
    extents: &[policy::Extent],
    read: bool,
) -> crate::EnsureOutcome {
    let mut provider = NoRemote(0);
    let mut downloads = 0;
    let mut fetch = |_: &BundleReceipt, _: &Path| {
        downloads += 1;
        Err("download forbidden".into())
    };
    let mut hosts = Hosts {
        provider: &mut provider,
        fetch: &mut fetch,
    };
    let result = crate::ensure(
        root,
        &scope(),
        &source.dataset(),
        &DeclaredSource::LocalAnalytical(Box::new(source.clone())),
        &policy::buffer_policy(&[200.]).unwrap(),
        extents,
        &[],
        if read {
            Mode::Read
        } else {
            Mode::Acquire(&mut hosts)
        },
    )
    .unwrap();
    assert_eq!((provider.0, downloads), (0, 0));
    result
}

#[test]
fn engine_pages_become_a_retained_indexed_source_reused_offline() {
    for origin in [Origin::GoogleOpenBuildings, Origin::MicrosoftBuildings] {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("held");
        let input = input(dir.path(), origin);
        let mut engine = Engine::new(vec![vec![footprint("microsoft:a", 30.0005)]]);
        counts::take();
        let cold = produce(&root, &scope(), &input, &mut |r| engine.convert(r)).unwrap();
        assert_eq!((engine.calls, counts::take()), (1, 1));
        let expected = if origin == Origin::GoogleOpenBuildings {
            Publisher::GoogleCsv
        } else {
            Publisher::MicrosoftGeojsonl
        };
        assert_eq!(engine.seen[0].publisher, expected);
        assert_eq!(engine.seen[0].provider, REFERENCE_CACHE_PROVIDER);
        assert_eq!(engine.seen[0].cells, coverage().cells);
        assert_eq!(cold.receipt["engine_conversion"], FOOTPRINT_CONVERSION);
        assert_eq!(cold.receipt["network_requests"], 0);
        let mut reader = ds_columnar::ArtifactReader::open(&cold.source.artifact_path, 16).unwrap();
        assert_eq!(reader.summary().feature_count, 1);
        let batch = reader.next_batch().unwrap().unwrap();
        assert_eq!(
            ds_columnar::batch_to_features(&batch, 1000).unwrap()[0]["id"],
            "microsoft:a"
        );
        let extents = vec![policy::Extent {
            name: "first-transformer".into(),
            bounds: Some([30., -2., 30.001, -1.999]),
        }];
        let first = seed(&root, &cold.source, &extents, false);
        assert!(first.covered && first.acquired > 0);
        assert_eq!(first.features, 1);
        std::fs::remove_file(&input.resources[0].path).unwrap();
        let retained = produce(&root, &scope(), &input, &mut |r| engine.convert(r)).unwrap();
        assert!(retained.reused);
        assert_eq!(
            (engine.calls, counts::take()),
            (1, 0),
            "no object or engine read"
        );
        assert_eq!(retained.source, cold.source);
        assert_eq!(seed(&root, &retained.source, &extents, false).acquired, 0);
        assert!(seed(&root, &retained.source, &extents, true).covered);
    }
}

#[test]
fn an_empty_completed_extract_is_a_reusable_source() {
    let dir = tempfile::tempdir().unwrap();
    let input = input(dir.path(), Origin::MicrosoftBuildings);
    let root = dir.path().join("held");
    let mut engine = Engine::new(vec![vec![]]);
    let out = produce(&root, &scope(), &input, &mut |r| engine.convert(r)).unwrap();
    assert_eq!(out.receipt["feature_count"], 0);
    let extents = vec![policy::Extent {
        name: "empty".into(),
        bounds: Some([30., -2., 30.001, -1.999]),
    }];
    let result = seed(&root, &out.source, &extents, false);
    assert!(result.covered);
    assert_eq!(result.features, 0);
}

#[test]
fn bad_pins_gaps_and_wrong_projects_refuse_before_the_engine() {
    let dir = tempfile::tempdir().unwrap();
    let mut engine = Engine::new(vec![]);
    let mut pinned = input(dir.path(), Origin::MicrosoftBuildings);
    pinned.resources[0].sha256 = "a".repeat(64);
    let error = produce(&dir.path().join("held"), &scope(), &pinned, &mut |r| {
        engine.convert(r)
    })
    .unwrap_err();
    assert_eq!(error.code(), "publisher_input_invalid");
    let mut gap = input(dir.path(), Origin::MicrosoftBuildings);
    gap.coverage.cells.push([31., -2.1, 31.2, -1.9]);
    assert!(matches!(
        produce(&dir.path().join("held"), &scope(), &gap, &mut |r| engine
            .convert(r)),
        Err(Error::Required(_))
    ));
    let mut other = scope();
    other.project = "other-project".into();
    assert!(matches!(
        produce(
            &dir.path().join("held"),
            &other,
            &input(dir.path(), Origin::MicrosoftBuildings),
            &mut |r| engine.convert(r)
        ),
        Err(Error::Invalid(_))
    ));
    assert_eq!(engine.calls, 0);
}

#[test]
fn engine_refusals_and_untrustworthy_pages_publish_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("held");
    let input = input(dir.path(), Origin::MicrosoftBuildings);
    let refused = produce(&root, &scope(), &input, &mut |_| {
        Err(Error::from_engine(
            "publisher_geometry_unsupported: publisher footprints admit Polygon and MultiPolygon only",
        ))
    })
    .unwrap_err();
    assert_eq!(refused.code(), "publisher_geometry_unsupported");
    let mut conflict = footprint("microsoft:same", 30.0005);
    conflict["properties"]["publisher_properties"] = json!("{\"height\":2}");
    let mut engine = Engine::new(vec![vec![footprint("microsoft:same", 30.0005), conflict]]);
    let error = produce(&root, &scope(), &input, &mut |r| engine.convert(r)).unwrap_err();
    assert_eq!(error.code(), "publisher_input_malformed");
    let mut lying = Engine::new(vec![vec![footprint("microsoft:a", 30.0005)]]);
    let error = produce(&root, &scope(), &input, &mut |request| {
        let mut result = lying.convert(request)?;
        result.pages[0].sha256 = "0".repeat(64);
        Ok(result)
    })
    .unwrap_err();
    assert_eq!(error.code(), "engine_response_invalid");
    assert!(!root
        .join("publisher-footprints")
        .join(input.identity())
        .exists());
}

#[test]
fn concurrent_consumers_share_one_conversion() {
    let dir = tempfile::tempdir().unwrap();
    let input = input(dir.path(), Origin::MicrosoftBuildings);
    let root = dir.path().join("held");
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
    let jobs: Vec<_> = (0..2)
        .map(|_| {
            let (input, root, barrier) = (input.clone(), root.clone(), barrier.clone());
            std::thread::spawn(move || {
                let mut engine = Engine::new(vec![vec![footprint("microsoft:one", 30.0005)]]);
                barrier.wait();
                let out = produce(&root, &scope(), &input, &mut |r| engine.convert(r)).unwrap();
                (out.reused, engine.calls)
            })
        })
        .collect();
    let results: Vec<_> = jobs.into_iter().map(|j| j.join().unwrap()).collect();
    assert_eq!(results.iter().map(|r| r.1).sum::<usize>(), 1);
    assert_eq!(results.iter().filter(|r| r.0).count(), 1);
}

/// The Network engine now writes the footprints artifact. It must be the very
/// bytes the in-process streamed writer wrote for the same merged features in
/// the same 512-feature appends, so a retained source reads identically.
#[test]
fn the_engine_writes_the_bytes_the_in_process_writer_wrote() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("held");
    let input = input(dir.path(), Origin::MicrosoftBuildings);
    let rows: Vec<Value> = (0..1300)
        .map(|i| footprint(&format!("microsoft:{i:05}"), 30.0 + f64::from(i) * 1e-5))
        .collect();
    let mut engine = Engine::new(vec![rows.clone()]);
    let produced = produce(&root, &scope(), &input, &mut |r| engine.convert(r)).unwrap();
    let written = std::fs::read(&produced.source.artifact_path).unwrap();

    let merged: BTreeMap<String, Value> = rows
        .into_iter()
        .map(|feature| (feature["id"].as_str().unwrap().to_owned(), feature))
        .collect();
    let local = dir.path().join("in-process.parquet");
    let mut writer = ds_columnar::ConversionStream::create(
        &local,
        ds_columnar::Provenance {
            source_digest: Some(sha(input.pins().to_string().as_bytes())),
            conversion: Some(input.identity()),
            ..Default::default()
        },
    )
    .unwrap();
    let features: Vec<Value> = merged.into_values().collect();
    for batch in features.chunks(BATCH) {
        writer.append(batch, &[]).unwrap();
    }
    writer.finish(&input.asset_id).unwrap();
    assert_eq!(written, std::fs::read(&local).unwrap());
    assert_eq!(produced.receipt["feature_count"], 1300);
    assert_eq!(produced.receipt["artifact_bytes"], written.len());
}
