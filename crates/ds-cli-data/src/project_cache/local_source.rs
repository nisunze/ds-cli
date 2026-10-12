//! Transport for the native local-source owner. No publisher or cloud door is
//! lent to this path. Project authorization remains the existing seed caller's.
use super::*;
use ds_project_data::{Declared, DeclaredSource, local_source::Source};
use std::io::Read;

const MAX_MANIFEST_BYTES: u64 = 64 * 1024;

/// The same lazy dispatch is used by seed, status and query. In particular,
/// selecting a local input never evaluates the catalogue-backed continuation.
pub(super) fn dispatch<T>(
    inputs: &Inputs,
    scope: &Scope,
    local: impl FnOnce(Declared) -> Result<T, Failure>,
    canonical: impl FnOnce() -> Result<T, Failure>,
) -> Result<T, Failure> {
    let Some(path) = inputs.value("local-source") else {
        return canonical();
    };
    let invalid = |message: String| {
        Failure::invalid(INVALID_SCOPE.code, message).remedy(
            "supply one native local-source manifest for this exact --project and selected asset revision",
        )
    };
    let metadata = std::fs::symlink_metadata(path)
        .map_err(|error| invalid(format!("--local-source: {error}")))?;
    if metadata.file_type().is_symlink()
        || !metadata.is_file()
        || metadata.len() > MAX_MANIFEST_BYTES
    {
        return Err(invalid(
            "--local-source must be a regular JSON file of at most 64 KiB".into(),
        ));
    }
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .and_then(|file| file.take(MAX_MANIFEST_BYTES + 1).read_to_end(&mut bytes))
        .map_err(|error| invalid(format!("--local-source: {error}")))?;
    if bytes.len() as u64 > MAX_MANIFEST_BYTES {
        return Err(invalid("--local-source grew beyond 64 KiB".into()));
    }
    let source: Source = serde_json::from_slice(&bytes)
        .map_err(|error| invalid(format!("--local-source: {error}")))?;
    source
        .validate(scope)
        .map_err(|error| invalid(error.message().to_owned()))?;
    let dataset = source.dataset();
    let explicit = explicit_dataset(inputs)?;
    if !explicit.is_empty() && explicit != dataset.id {
        return Err(invalid(format!(
            "--dataset {explicit} differs from local source {}",
            dataset.id
        )));
    }
    local(Declared {
        candidate: SeedCandidate {
            id: dataset.id.clone(),
            label: source.asset_id.clone(),
            seeded: false,
        },
        dataset,
        source: DeclaredSource::LocalAnalytical(Box::new(source)),
    })
}

pub(super) fn status(
    root: &Path,
    scope: &Scope,
    lane: &str,
    entry: &Declared,
    summary: bool,
    mut verify_scope: impl FnMut() -> Result<(), Failure>,
) -> Result<Value, Failure> {
    verify_scope()?;
    let held = ds_layer_store::project_dataset_cache::execute_at(
        root,
        json!({"action":"status", "dataset_key":policy::dataset_key(scope, &entry.dataset),
            "principal":scope.principal,"project":scope.project}),
    )
    .map_err(|error| Failure::unavailable(STORE_FAILED.code, error))?;
    let policy =
        buffer_policy(&[]).map_err(|error| Failure::unavailable(STORE_FAILED.code, error))?;
    let mut answer = policy::overview(
        &scope.project,
        &policy,
        &[OverviewRow {
            dataset: catalog_entry(&entry.dataset, &entry.candidate.label, false),
            held,
        }],
    );
    answer["lane"] = json!(lane);
    answer["catalog"] = json!({"read":false,"reason":"explicit_local_source"});
    answer["selected_source"] = metadata(entry);
    verify_scope()?;
    if summary {
        Ok(compact_status(answer))
    } else {
        Ok(answer)
    }
}

pub(super) fn metadata(entry: &Declared) -> Value {
    match &entry.source {
        DeclaredSource::LocalAnalytical(source) => json!({
            "asset_id":source.asset_id,"revision":source.revision,
            "actual_source":source.actual_source,"producer_source":source.producer_source,
            "artifact_sha256":source.artifact_sha256,"source_version":source.identity().version,
        }),
        _ => Value::Null,
    }
}

struct NoRemote;
impl Provider for NoRemote {
    fn acquire(&mut self, _: &Dataset, _: &Value) -> Result<Acquisition, ds_project_data::Failure> {
        #[cfg(test)]
        counts::provider();
        Err(ds_project_data::Failure::Unsupported(
            "an explicit local source cannot acquire from a cloud provider".into(),
        ))
    }
}

/// The native owner decides gaps, geometry, deduplication and commit fences.
/// This adapter supplies only selected inputs and transports its result.
pub(super) fn seed(
    root: &Path,
    scope: &Scope,
    entry: &Declared,
    extents: &[policy::Extent],
    refresh: bool,
    mut verify_scope: impl FnMut() -> Result<(), Failure>,
) -> Result<Value, Failure> {
    verify_scope()?;
    let mut policy =
        buffer_policy(&[]).map_err(|error| Failure::unavailable(STORE_FAILED.code, error))?;
    policy.sheets = both_orientations();
    let mut provider = NoRemote;
    let mut fetch = |_: &BundleReceipt, _: &Path| {
        #[cfg(test)]
        counts::bundle();
        Err("an explicit local source cannot download a reference bundle".into())
    };
    let mut hosts = Hosts {
        provider: &mut provider,
        fetch: &mut fetch,
    };
    let outcome = if refresh {
        ds_project_data::refresh(
            root,
            scope,
            &entry.dataset,
            &entry.source,
            &policy,
            extents,
            &[],
            &mut hosts,
        )
    } else {
        ds_project_data::ensure(
            root,
            scope,
            &entry.dataset,
            &entry.source,
            &policy,
            extents,
            &[],
            Mode::Acquire(&mut hosts),
        )
    }
    .map_err(refused)?;
    verify_scope()?;
    let mut row = json!({
        "dataset_id":entry.dataset.id,"label":entry.candidate.label,
        "dataset_key":outcome.dataset_key,"clusters":outcome.clusters,
        "acquired":outcome.acquired,"skipped":outcome.skipped,"covered":outcome.covered,
        "feature_count":outcome.features,"local_holding":true,"shared":false,
        "coverage":outcome.coverage,"warnings":outcome.warnings,
        "selected_source":metadata(entry),
    });
    if !outcome.covered {
        row["error"] =
            json!("coverage is not complete; another acquisition may still be preparing this area");
    }
    Ok(row)
}

#[cfg(test)]
mod counts {
    use std::cell::Cell;
    thread_local! { static COUNTS: Cell<(usize,usize)> = const { Cell::new((0,0)) }; }
    pub fn provider() {
        COUNTS.with(|c| {
            let (p, b) = c.get();
            c.set((p + 1, b));
        });
    }
    pub fn bundle() {
        COUNTS.with(|c| {
            let (p, b) = c.get();
            c.set((p, b + 1));
        });
    }
    pub fn take() -> (usize, usize) {
        COUNTS.with(|c| c.replace((0, 0)))
    }
}

#[cfg(test)]
mod tests {
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
}
