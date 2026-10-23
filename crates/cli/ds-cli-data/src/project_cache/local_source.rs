//! Transport for the native local-source owner. No publisher or cloud door is
//! lent to this path. Project authorization remains the existing seed caller's.
use super::*;
use ds_project_data::{Declared, DeclaredSource, local_source::Source};
use std::io::Read;

const MAX_MANIFEST_BYTES: u64 = 64 * 1024;

#[derive(Debug)]
pub(super) enum Selected {
    Analytical(Declared),
    Publisher(ds_project_data::publisher_footprints::Input),
}

pub(super) fn select_seed(inputs: &Inputs, scope: &Scope) -> Result<Option<Selected>, Failure> {
    let Some(path) = inputs.value("publisher-input") else {
        return dispatch(
            inputs,
            scope,
            |entry| Ok(Some(Selected::Analytical(entry))),
            || Ok(None),
        );
    };
    if inputs.value("local-source").is_some() || !explicit_dataset(inputs)?.is_empty() {
        return Err(Failure::invalid(PUBLISHER_INVALID.code,
            "--publisher-input selects its own native asset dataset; do not combine it with --local-source or --dataset")
            .remedy(PUBLISHER_INVALID.remedy));
    }
    let invalid = |error: String| {
        Failure::invalid(PUBLISHER_INVALID.code, error).remedy(PUBLISHER_INVALID.remedy)
    };
    let meta = std::fs::symlink_metadata(path).map_err(|e| invalid(e.to_string()))?;
    if !meta.is_file() || meta.file_type().is_symlink() || meta.len() > MAX_MANIFEST_BYTES {
        return Err(invalid(
            "--publisher-input must be a regular JSON file of at most 64 KiB".into(),
        ));
    }
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .and_then(|f| f.take(MAX_MANIFEST_BYTES + 1).read_to_end(&mut bytes))
        .map_err(|e| invalid(e.to_string()))?;
    if bytes.len() as u64 > MAX_MANIFEST_BYTES {
        return Err(invalid("--publisher-input grew beyond 64 KiB".into()));
    }
    let input: ds_project_data::publisher_footprints::Input =
        serde_json::from_slice(&bytes).map_err(|e| invalid(e.to_string()))?;
    input.validate(scope).map_err(publisher_refused)?;
    Ok(Some(Selected::Publisher(input)))
}

pub(super) fn materialize(
    root: &Path,
    scope: &Scope,
    selected: Selected,
    convert: &mut ds_project_data::publisher_footprints::Convert<'_>,
) -> Result<(Declared, Value), Failure> {
    match selected {
        Selected::Analytical(entry) => Ok((entry, Value::Null)),
        Selected::Publisher(input) => {
            // The Reporter normalizes the rows (the host lends it as
            // `convert`); a retained conversion never reaches it.
            let out = ds_project_data::publisher_footprints::produce(root, scope, &input, convert)
                .map_err(publisher_refused)?;
            let dataset = out.source.dataset();
            let receipt = json!({"source_manifest":out.source_manifest,"reused":out.reused,
                "actual_source":out.source.actual_source,"publisher_revision":input.publisher_revision,
                "producer_source":out.source.producer_source,"feature_count":out.receipt["feature_count"],
                "brain_compute_requests":0});
            Ok((
                Declared {
                    candidate: SeedCandidate {
                        id: dataset.id.clone(),
                        label: out.source.asset_id.clone(),
                        seeded: false,
                    },
                    dataset,
                    source: DeclaredSource::LocalAnalytical(Box::new(out.source)),
                },
                receipt,
            ))
        }
    }
}

fn publisher_refused(error: ds_project_data::publisher_footprints::Error) -> Failure {
    use ds_project_data::publisher_footprints::Error;
    let message = error.message().to_owned();
    match error {
        Error::Invalid(_) => {
            Failure::invalid(PUBLISHER_INVALID.code, message).remedy(PUBLISHER_INVALID.remedy)
        }
        Error::Required(_) => Failure::conflict(LOCAL_RESOURCE_REQUIRED.code, message)
            .remedy(LOCAL_RESOURCE_REQUIRED.remedy),
        Error::Malformed(_) => {
            Failure::failed(PUBLISHER_MALFORMED.code, message).remedy(PUBLISHER_MALFORMED.remedy)
        }
        Error::Unsupported(_) => Failure::invalid(PUBLISHER_UNSUPPORTED.code, message)
            .remedy(PUBLISHER_UNSUPPORTED.remedy),
        Error::TooLarge(_) => {
            Failure::invalid(PUBLISHER_TOO_LARGE.code, message).remedy(PUBLISHER_TOO_LARGE.remedy)
        }
        Error::Store(_) => {
            Failure::unavailable(STORE_FAILED.code, message).remedy(STORE_FAILED.remedy)
        }
        // The converting engine's own typed refusal (missing, incompatible,
        // refused or an invalid answer), declared on seed.
        Error::Engine(code, _) => {
            let declared = [
                REPORTER_MISSING,
                REPORTER_INCOMPATIBLE,
                ENGINE_REFUSED,
                ENGINE_RESPONSE_INVALID,
            ]
            .into_iter()
            .find(|refusal| refusal.code == code)
            .unwrap_or(ENGINE_REFUSED);
            Failure::unavailable(declared.code, message).remedy(declared.remedy)
        }
    }
}

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
#[path = "local_source_tests.rs"]
mod tests;
