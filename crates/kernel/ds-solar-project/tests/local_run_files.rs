//! Local run admission and commitment stay usable without a calculation engine.
use ds_solar_reference_host::ReferencePreparer;
#[path = "../../ds-solar-reference-host/tests/common/mod.rs"]
mod common;

use ds_solar_contracts::{
    ArtifactRole, BatchArtifact, BatchCity, BatchRunIdentity, PreparedSolarCityInput,
    SolarArtifactBatch, SolarError, digest,
};
use ds_solar_io::cache::{DirectoryReferenceCache, ReferenceCache};
use ds_solar_project::{
    files::{self, RunFailure, commit_artifact_run},
    preparation::{MAX_PREPARED_INPUT_BYTES, load_prepared, load_prepared_for_run},
};
use serde_json::json;
use std::{collections::BTreeMap, path::Path};

fn prepared(city: &str) -> PreparedSolarCityInput {
    let temp = tempfile::tempdir().unwrap();
    let cache = DirectoryReferenceCache::new(temp.path());
    cache
        .put(&common::reference_key(city), &common::reference_bytes(city))
        .unwrap();
    ReferencePreparer::new()
        .with_cache(&cache)
        .prepare(common::city_input(city), common::PREPARED_AT)
        .unwrap()
        .input
}
fn save(path: &Path, input: &PreparedSolarCityInput) {
    std::fs::write(path, serde_json::to_vec_pretty(input).unwrap()).unwrap();
}

#[test]
fn prepared_selection_is_sorted_exact_and_does_not_consume_claims() {
    let temp = tempfile::tempdir().unwrap();
    let bere = prepared("aderm_bere");
    let beinamar = prepared("aderm_beinamar");
    save(&temp.path().join("a.prepared.json"), &bere);
    save(&temp.path().join("z.prepared.json"), &beinamar);
    std::fs::write(
        temp.path().join("aderm_bere.claim.json"),
        b"not publication authority",
    )
    .unwrap();
    let all = load_prepared_for_run(temp.path(), &[]).unwrap();
    assert_eq!(
        all.iter()
            .map(|p| p.city.identity.city_id.as_str())
            .collect::<Vec<_>>(),
        ["aderm_beinamar", "aderm_bere"]
    );
    let selected =
        load_prepared_for_run(temp.path(), &["aderm_bere".into(), "aderm_beinamar".into()])
            .unwrap();
    assert_eq!(
        serde_json::to_vec(&all).unwrap(),
        serde_json::to_vec(&selected).unwrap()
    );
    assert_eq!(
        serde_json::to_vec(&all[1]).unwrap(),
        serde_json::to_vec(&bere).unwrap()
    );
    assert_eq!(
        std::fs::read(temp.path().join("aderm_bere.claim.json")).unwrap(),
        b"not publication authority"
    );
}

#[test]
fn duplicate_missing_unsafe_and_unselected_duplicate_inputs_refuse() {
    let temp = tempfile::tempdir().unwrap();
    let bere = prepared("aderm_bere");
    save(&temp.path().join("bere.prepared.json"), &bere);
    for wanted in [
        vec!["aderm_bere".into(), "aderm_bere".into()],
        vec!["aderm_bere".into(), "missing".into()],
        vec!["../bere".into()],
    ] {
        assert!(load_prepared_for_run(temp.path(), &wanted).is_err());
    }
    let mut invalid = bere.clone();
    invalid.city.identity.city_id = "../outside".into();
    save(&temp.path().join("unsafe.prepared.json"), &invalid);
    assert!(
        load_prepared_for_run(temp.path(), &["aderm_bere".into()])
            .unwrap_err()
            .to_string()
            .contains("validating")
    );
    std::fs::remove_file(temp.path().join("unsafe.prepared.json")).unwrap();
    let beinamar = prepared("aderm_beinamar");
    save(&temp.path().join("other.prepared.json"), &beinamar);
    save(&temp.path().join("duplicate.prepared.json"), &beinamar);
    assert!(
        load_prepared_for_run(temp.path(), &["aderm_bere".into()])
            .unwrap_err()
            .to_string()
            .contains("repeats city")
    );
}

#[test]
fn selected_inputs_all_require_readiness_before_a_run_and_unselected_seals_are_not_run() {
    let temp = tempfile::tempdir().unwrap();
    save(
        &temp.path().join("a.prepared.json"),
        &prepared("aderm_beinamar"),
    );
    let mut changed = prepared("aderm_bere");
    changed.input_digest = "sha256:tampered".into();
    save(&temp.path().join("z.prepared.json"), &changed);
    assert!(
        load_prepared(temp.path(), &[]).is_ok(),
        "bench loading keeps its prior admission boundary"
    );
    assert!(
        load_prepared_for_run(temp.path(), &[])
            .unwrap_err()
            .to_string()
            .starts_with("aderm_bere:")
    );
    assert!(load_prepared_for_run(temp.path(), &["aderm_beinamar".into()]).is_ok());
    assert_eq!(std::fs::read_dir(temp.path()).unwrap().count(), 2);
}

#[test]
fn empty_nonregular_malformed_and_oversized_prepared_inputs_refuse() {
    let temp = tempfile::tempdir().unwrap();
    assert!(
        load_prepared_for_run(temp.path(), &[])
            .unwrap_err()
            .to_string()
            .contains("no prepared inputs")
    );
    let path = temp.path().join("input.prepared.json");
    std::fs::create_dir(&path).unwrap();
    assert!(load_prepared_for_run(temp.path(), &[]).is_err());
    std::fs::remove_dir(&path).unwrap();
    std::fs::write(&path, b"{").unwrap();
    assert!(
        load_prepared_for_run(temp.path(), &[])
            .unwrap_err()
            .to_string()
            .contains("parsing")
    );
    std::fs::File::create(&path)
        .unwrap()
        .set_len(MAX_PREPARED_INPUT_BYTES + 1)
        .unwrap();
    assert!(
        load_prepared_for_run(temp.path(), &[])
            .unwrap_err()
            .to_string()
            .contains("read bound")
    );
}

fn artifacts() -> (SolarArtifactBatch, BTreeMap<String, Vec<u8>>) {
    let bytes: BTreeMap<String, Vec<u8>> = BTreeMap::from([
        (
            "cities/bere/result.json".into(),
            b"{\n  \"fixture\": true\n}\n".to_vec(),
        ),
        (
            "cities/bere/maps/map.png".into(),
            b"\x89PNG\r\n\x1a\nlocal-artifact".to_vec(),
        ),
    ]);
    let batch = SolarArtifactBatch::close(
        BatchRunIdentity {
            project_id: "local-test".into(),
            root: "eds_project/local-test/eds_solar".into(),
            run_id: "run-1".into(),
            cities: vec![BatchCity {
                city_id: "bere".into(),
                input_digest: digest::sha256_hex(b"input"),
                result_digest: digest::sha256_hex(b"result"),
            }],
        },
        bytes
            .iter()
            .map(|(name, value)| BatchArtifact {
                name: name.clone(),
                role: if name.ends_with("json") {
                    ArtifactRole::CityResult
                } else {
                    ArtifactRole::Export
                },
                media_type: if name.ends_with("json") {
                    "application/json"
                } else {
                    "image/png"
                }
                .into(),
                byte_count: value.len() as u64,
                content_digest: digest::sha256_hex(value),
                city_id: Some("bere".into()),
            })
            .collect(),
    )
    .unwrap();
    (batch, bytes)
}
fn stage(root: &Path, bytes: &BTreeMap<String, Vec<u8>>) -> anyhow::Result<()> {
    for (name, value) in bytes {
        files::write(&root.join(name), value)?;
    }
    Ok(())
}
fn pretty(value: &impl serde::Serialize) -> Vec<u8> {
    let mut bytes = serde_json::to_vec_pretty(value).unwrap();
    bytes.push(b'\n');
    bytes
}

#[test]
fn commits_exact_artifact_and_legacy_sidecar_bytes_for_partial_success() {
    let temp = tempfile::tempdir().unwrap();
    let out = temp.path().join("run");
    let (batch, bytes) = artifacts();
    let error = SolarError::invalid_input("failed city fixture");
    let failures = [RunFailure {
        city_id: "failed",
        error: &error,
    }];
    let progress = json!([{ "stage":"fixture", "message":"done" }]);
    commit_artifact_run(&out, &batch, &failures, Some(&progress), |root| {
        stage(root, &bytes)
    })
    .unwrap();
    files::verify(&out, &batch).unwrap();
    for (name, value) in bytes {
        assert_eq!(std::fs::read(out.join(name)).unwrap(), value);
    }
    assert_eq!(
        std::fs::read(out.join("batch.json")).unwrap(),
        pretty(&batch)
    );
    assert_eq!(
        std::fs::read(out.join("failures.json")).unwrap(),
        pretty(&vec![json!({"city_id":"failed", "error":error})])
    );
    assert_eq!(
        std::fs::read(out.join("progress.json")).unwrap(),
        pretty(&progress)
    );
    let intent: serde_json::Value =
        serde_json::from_slice(&std::fs::read(out.join("sync-intent.json")).unwrap()).unwrap();
    assert_eq!(
        std::fs::read(out.join("sync-intent.json")).unwrap(),
        pretty(&batch.sync_intent(intent["enqueued_at"].as_str().unwrap()))
    );
    assert!(!out.join("cities/failed").exists());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&out).unwrap().permissions().mode() & 0o777,
            0o700
        );
        assert_eq!(
            std::fs::metadata(out.join("batch.json"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }
}

#[test]
fn invalid_closure_missing_and_tampered_bytes_never_publish() {
    let temp = tempfile::tempdir().unwrap();
    let out = temp.path().join("run");
    let (mut batch, mut bytes) = artifacts();
    batch.batch_digest = digest::sha256_hex(b"tampered");
    assert!(
        commit_artifact_run(&out, &batch, &[], None, |_| panic!(
            "invalid batch admitted"
        ))
        .is_err()
    );
    let (batch, _) = artifacts();
    assert!(commit_artifact_run(&out, &batch, &[], None, |_| Ok(())).is_err());
    bytes.values_mut().next().unwrap().push(b'!');
    assert!(commit_artifact_run(&out, &batch, &[], None, |root| stage(root, &bytes)).is_err());
    assert!(!out.exists());
    assert_eq!(std::fs::read_dir(temp.path()).unwrap().count(), 0);
}

#[test]
fn existing_destination_refuses_before_staging_and_raced_destination_is_preserved() {
    let temp = tempfile::tempdir().unwrap();
    let out = temp.path().join("run");
    let (batch, bytes) = artifacts();
    std::fs::create_dir(&out).unwrap();
    assert!(
        commit_artifact_run(&out, &batch, &[], None, |_| panic!(
            "existing destination admitted"
        ))
        .is_err()
    );
    std::fs::remove_dir(&out).unwrap();
    let mut staged = None;
    let error = commit_artifact_run(&out, &batch, &[], None, |root| {
        stage(root, &bytes)?;
        staged = Some(root.to_path_buf());
        std::fs::create_dir(&out)?; // The former fs::rename replaced exactly this empty directory.
        Ok(())
    })
    .unwrap_err();
    assert!(error.to_string().contains("inspect"), "{error:#}");
    assert_eq!(
        std::fs::read_dir(&out).unwrap().count(),
        0,
        "raced destination was overwritten"
    );
    let candidate = staged.unwrap();
    assert!(error.to_string().contains(candidate.to_str().unwrap()));
    files::verify(&candidate, &batch).unwrap();
    assert!(candidate.join("batch.json").is_file());
    assert!(candidate.join("sync-intent.json").is_file());
}

#[test]
fn empty_success_set_keeps_failures_and_local_intent_without_progress() {
    let temp = tempfile::tempdir().unwrap();
    let (mut batch, _) = artifacts();
    batch.run.cities.clear();
    let batch = SolarArtifactBatch::close(batch.run, vec![]).unwrap();
    let error = SolarError::invalid_input("all failed fixture");
    let out = temp.path().join("none-succeeded");
    commit_artifact_run(
        &out,
        &batch,
        &[RunFailure {
            city_id: "bere",
            error: &error,
        }],
        None,
        |_| Ok(()),
    )
    .unwrap();
    assert_eq!(std::fs::read_dir(&out).unwrap().count(), 3);
    assert!(!out.join("progress.json").exists());
    assert_eq!(
        std::fs::read(out.join("failures.json")).unwrap(),
        pretty(&vec![json!({"city_id":"bere", "error":error})])
    );
}

#[cfg(unix)]
#[test]
fn existing_output_link_refuses_and_parent_aliases_keep_their_local_meaning() {
    use std::os::unix::fs::symlink;
    let temp = tempfile::tempdir().unwrap();
    let (batch, bytes) = artifacts();
    let outside = temp.path().join("outside");
    std::fs::create_dir(&outside).unwrap();
    let out = temp.path().join("alias");
    symlink(&outside, &out).unwrap();
    assert!(
        commit_artifact_run(&out, &batch, &[], None, |_| panic!(
            "existing output link admitted"
        ))
        .is_err()
    );
    assert_eq!(std::fs::read_dir(&outside).unwrap().count(), 0);
    let output = out.join("child/../run");
    commit_artifact_run(&output, &batch, &[], None, |root| stage(root, &bytes)).unwrap();
    files::verify(&outside.join("run"), &batch).unwrap();
}
