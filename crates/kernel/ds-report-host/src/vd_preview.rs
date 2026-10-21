//! Fixed A4 saved-analysis admission over the common native file preview room.
//! SVG and PDF share one disk artifact/index/expiry/reader owner. This wrapper
//! only names the exact saved engineering and renderer inputs; it has no store,
//! process, transport, analysis or publication implementation.
use crate::{preview_artifact, HostFailure};
use ds_project_data::preview_room::Room;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Inputs {
    pub transformer: String,
    pub transformer_revision: i64,
    pub analysis_sha256: String,
    pub content_digest: String,
    pub settings_sha256: String,
    pub engine_version: String,
    pub engine_build_manifest_sha256: String,
}
pub type Lease = preview_artifact::RequestLease;
pub type Receipt = preview_artifact::RequestReceipt;
fn failure(message: impl Into<String>) -> HostFailure {
    HostFailure::new("print_preview_artifact_unavailable", message)
}
fn validate(inputs: &Inputs) -> Result<(), HostFailure> {
    if !ds_command_kernel::report_export::valid_transformer_identity(&inputs.transformer)
        || inputs.transformer_revision <= 0
        || inputs.engine_version.trim().is_empty()
        || inputs.engine_version.len() > 256
        || ![
            &inputs.analysis_sha256,
            &inputs.content_digest,
            &inputs.settings_sha256,
            &inputs.engine_build_manifest_sha256,
        ]
        .into_iter()
        .all(|s| ds_command_kernel::report_export::exact_lower_sha256(s))
    {
        return Err(HostFailure::new(
            crate::CODE_INPUTS_INVALID,
            "A4 preview requires exact saved analysis, revision, settings and engine pins",
        ));
    }
    Ok(())
}
pub fn provenance(inputs: &Inputs) -> Result<Value, HostFailure> {
    validate(inputs)?;
    Ok(json!({"subject":"saved-analysis-voltage-drop-a4","pins":inputs}))
}
/// Hosts admit saved source bytes before lookup. The common disk lease joins
/// overlapping native requests while the actual renderer continues after view
/// navigation. No unused room or cloud freshness is inspected.
pub fn begin(
    room: &Room,
    inputs: Inputs,
    retention: preview_artifact::Retention,
) -> Result<Lease, HostFailure> {
    preview_artifact::begin_request(room, provenance(&inputs)?, retention)
}
#[cfg(test)]
fn begin_at(
    room: &Room,
    inputs: Inputs,
    retention: preview_artifact::Retention,
    time: u64,
) -> Result<Lease, HostFailure> {
    preview_artifact::begin_request_at(room, provenance(&inputs)?, retention, time)
}
pub fn materialize(
    room: &Room,
    inputs: &Inputs,
    pdf: &mut std::fs::File,
    size: u64,
    sha: &str,
    retention: preview_artifact::Retention,
) -> Result<preview_artifact::Artifact, HostFailure> {
    preview_artifact::materialize_pdf_retained(room, pdf, size, sha, provenance(inputs)?, retention)
}
pub fn verify(
    room: &Room,
    inputs: &Inputs,
    artifact: &preview_artifact::Artifact,
) -> Result<(), HostFailure> {
    if artifact.content_type != "application/pdf" {
        return Err(failure("A4 saved analysis requires a native PDF file"));
    }
    preview_artifact::verify(room, artifact, &provenance(inputs)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Barrier,
    };
    use std::{fs, path::PathBuf};
    struct Engine<'a>(&'a AtomicUsize);
    impl crate::ReportEngine for Engine<'_> {
        fn build_info(&self) -> Result<Value, HostFailure> {
            Ok(json!({}))
        }
        fn export_transformer_report(
            &self,
            _request: &std::path::Path,
            result: &std::path::Path,
        ) -> Result<crate::EngineExit, HostFailure> {
            self.0.fetch_add(1, Ordering::SeqCst);
            fs::write(result, b"%PDF-1.7 counted native A4 task")
                .map_err(|e| failure(e.to_string()))?;
            Ok(crate::EngineExit {
                succeeded: true,
                summary: String::new(),
            })
        }
    }
    fn room(root: PathBuf) -> Room {
        Room {
            root,
            scope: ds_command_kernel::project_dataset_cache::Scope {
                principal: "owner".into(),
                project: "project".into(),
            },
            lane: "canary".into(),
            credential_audience_sha256: "a".repeat(64),
        }
    }
    fn inputs() -> Inputs {
        Inputs {
            transformer: "tx_a".into(),
            transformer_revision: 7,
            analysis_sha256: "b".repeat(64),
            content_digest: "c".repeat(64),
            settings_sha256: "d".repeat(64),
            engine_version: "network-reporter@test".into(),
            engine_build_manifest_sha256: "e".repeat(64),
        }
    }
    fn consume(room: &Room, inputs: Inputs, time: u64, calls: &AtomicUsize) -> Value {
        let mut lease = begin_at(
            room,
            inputs.clone(),
            preview_artifact::Retention::Hours24,
            time,
        )
        .unwrap();
        if let Some(value) = lease.retained() {
            return value.clone();
        }
        // Count the actual fixed task invocation, with real file bytes; the
        // cache contains only its result locator and byte pins.
        let pdf = room.root.join(format!("{}.pdf", crate::random_run_id()));
        crate::ReportEngine::export_transformer_report(
            &Engine(calls),
            &room.root.join("request.json"),
            &pdf,
        )
        .unwrap();
        let sha = ds_command_kernel::report_export::sha256_hex(&fs::read(&pdf).unwrap());
        crate::verify_file(&pdf, fs::metadata(&pdf).unwrap().len(), &sha).unwrap();
        let artifact = preview_artifact::materialize_pdf_at(
            room,
            &fs::read(&pdf).unwrap(),
            provenance(&inputs).unwrap(),
            preview_artifact::Retention::Hours24,
            time,
        )
        .unwrap();
        let value = json!({"previewArtifact":artifact});
        lease.commit_at(value.clone(), &artifact, time).unwrap();
        value
    }
    #[test]
    fn cold_overlapping_and_restarted_consumers_join_one_disk_render() {
        let temp = tempfile::tempdir().unwrap();
        let room = room(temp.path().into());
        let calls = Arc::new(AtomicUsize::new(0));
        let barrier = Arc::new(Barrier::new(8));
        std::thread::scope(|threads| {
            for _ in 0..8 {
                let (room, calls, barrier) = (&room, calls.clone(), barrier.clone());
                threads.spawn(move || {
                    barrier.wait();
                    consume(room, inputs(), 100, &calls);
                });
            }
        });
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        // No coordinator holds result bytes: replacement reads the disk lease.
        consume(&room, inputs(), 1000, &calls);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }
    #[test]
    fn original_expiry_24h_and_7d_do_not_slide() {
        let temp = tempfile::tempdir().unwrap();
        let room = room(temp.path().into());
        let calls = AtomicUsize::new(0);
        consume(&room, inputs(), 100, &calls);
        consume(&room, inputs(), preview_artifact::RETENTION_MS + 99, &calls);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        consume(
            &room,
            inputs(),
            preview_artifact::RETENTION_MS + 100,
            &calls,
        );
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        let mut long = begin_at(&room, inputs(), preview_artifact::Retention::Days7, 100).unwrap();
        let artifact = preview_artifact::materialize_pdf_at(
            &room,
            b"%PDF-1.7 long",
            provenance(&inputs()).unwrap(),
            preview_artifact::Retention::Days7,
            100,
        )
        .unwrap();
        long.commit_at(json!({"previewArtifact":artifact}), &artifact, 100)
            .unwrap();
        drop(long);
        let long = begin_at(
            &room,
            inputs(),
            preview_artifact::Retention::Days7,
            preview_artifact::LONG_RETENTION_MS + 99,
        )
        .unwrap();
        assert!(long.retained().is_some());
        drop(long);
        let long = begin_at(
            &room,
            inputs(),
            preview_artifact::Retention::Days7,
            preview_artifact::LONG_RETENTION_MS + 100,
        )
        .unwrap();
        assert!(long.retained().is_none());
    }
    #[test]
    fn every_input_and_authority_dimension_separates_and_previous_hash_is_reusable() {
        let temp = tempfile::tempdir().unwrap();
        let base = room(temp.path().into());
        let calls = AtomicUsize::new(0);
        consume(&base, inputs(), 100, &calls);
        for field in 0..7 {
            let mut changed = inputs();
            match field {
                0 => changed.transformer = "tx_b".into(),
                1 => changed.transformer_revision += 1,
                2 => changed.analysis_sha256 = "1".repeat(64),
                3 => changed.content_digest = "2".repeat(64),
                4 => changed.settings_sha256 = "3".repeat(64),
                5 => changed.engine_version.push('2'),
                _ => changed.engine_build_manifest_sha256 = "4".repeat(64),
            };
            consume(&base, changed, 100, &calls);
        }
        for field in 0..4 {
            let mut changed = base.clone();
            match field {
                0 => changed.scope.project = "project2".into(),
                1 => changed.scope.principal = "owner2".into(),
                2 => changed.lane = "stable".into(),
                _ => changed.credential_audience_sha256 = "5".repeat(64),
            };
            consume(&changed, inputs(), 100, &calls);
        }
        assert_eq!(calls.load(Ordering::SeqCst), 12);
        consume(&base, inputs(), 200, &calls);
        assert_eq!(calls.load(Ordering::SeqCst), 12);
    }
    #[test]
    fn missing_source_and_corrupt_metadata_never_launch_render() {
        let temp = tempfile::tempdir().unwrap();
        let room = room(temp.path().into());
        let mut missing = inputs();
        missing.analysis_sha256.clear();
        assert!(begin_at(&room, missing, preview_artifact::Retention::Hours24, 100).is_err());
        let mut lease =
            begin_at(&room, inputs(), preview_artifact::Retention::Hours24, 100).unwrap();
        let artifact = preview_artifact::materialize_pdf_at(
            &room,
            b"%PDF-1.7 held",
            provenance(&inputs()).unwrap(),
            preview_artifact::Retention::Hours24,
            100,
        )
        .unwrap();
        lease
            .commit_at(json!({"previewArtifact":artifact}), &artifact, 100)
            .unwrap();
        let path = lease.path.clone();
        drop(lease);
        let mut value: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        value["metadata"]["pdf"] = json!("changed");
        fs::write(path, serde_json::to_vec(&value).unwrap()).unwrap();
        assert!(begin_at(&room, inputs(), preview_artifact::Retention::Hours24, 100).is_err());
    }
}
