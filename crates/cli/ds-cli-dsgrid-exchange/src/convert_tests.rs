use ds_cli_contract::outcome::ExitClass;

use super::*;

fn answer(status: &str) -> Value {
    json!({
        "plan_id": "plan:pinned",
        "status": status,
        "target": "pls_backup",
        "written": [],
        "written_count": 0,
        "report": "/out/exchange-report.json",
        "per_source": [{
            "source_index": 0,
            "name": "model.dsgrid",
            "status": "failed",
            "detail": "strict workspace export failed: 2 difference(s) fall outside this release's characterized export edit classes",
        }],
    })
}

/// A conversion that ran without producing what was asked for answers with a
/// failure envelope, never `ok` (the PLS `.bak` export that wrote nothing
/// used to report success). The written evidence travels in the detail, and
/// both codes are declared on the command.
#[test]
fn only_a_completed_outcome_is_a_success() {
    let completed = finish("completed", answer("completed")).unwrap();
    assert_eq!(completed["status"], "completed");

    for (status, code) in [
        ("failed", "conversion_failed"),
        ("partially_completed", "conversion_partial"),
    ] {
        let failure = finish(status, answer(status)).unwrap_err();
        assert_eq!(failure.code(), code);
        assert_eq!(failure.class(), ExitClass::Failed);
        assert!(
            failure
                .remedy_text()
                .is_some_and(|remedy| !remedy.is_empty())
        );
        let detail = failure
            .detail_value()
            .expect("the outcome travels as detail");
        assert_eq!(detail["report"], "/out/exchange-report.json");
        assert_eq!(detail["per_source"][0]["status"], "failed");
        assert!(
            COMMAND.refusals.iter().any(|refusal| refusal.code == code),
            "{code} is not declared"
        );
    }
    let unknown = finish("guessed", answer("guessed")).unwrap_err();
    assert_eq!(unknown.code(), "network_result_invalid");
}

fn stored(entries: &[(&str, &[u8])]) -> Vec<u8> {
    use std::io::Write as _;
    let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Stored);
    for (name, bytes) in entries {
        writer.start_file(*name, options).unwrap();
        writer.write_all(bytes).unwrap();
    }
    writer.finish().unwrap().into_inner()
}

fn declared(path: &str, bytes: &[u8]) -> ExchangeOutputV1 {
    ExchangeOutputV1 {
        path: path.into(),
        artifact_type: "pls_backup".into(),
        byte_len: bytes.len() as u64,
        sha256: sha256(bytes),
        source_index: Some(0),
    }
}

/// The archive is the addon's claim; every entry is held to its declaration
/// and to `--out` before anything is written.
#[test]
fn an_archive_that_differs_from_its_declaration_writes_nothing() {
    let outputs = vec![declared("m.bak", b"bak"), declared("lib/a.012", b"pole")];
    let good = stored(&[("m.bak", b"bak"), ("lib/a.012", b"pole")]);
    let files = verified_outputs(&outputs, Some(good)).unwrap();
    assert_eq!(files, vec![b"bak".to_vec(), b"pole".to_vec()]);

    for (outputs, archive) in [
        // A changed byte.
        (outputs.clone(), Some(stored(&[("m.bak", b"bax"), ("lib/a.012", b"pole")]))),
        // An undeclared extra entry.
        (
            vec![declared("m.bak", b"bak")],
            Some(stored(&[("m.bak", b"bak"), ("x", b"")])),
        ),
        // A declared output that is absent.
        (outputs.clone(), None),
        // Outputs nobody declared.
        (Vec::new(), Some(stored(&[("m.bak", b"bak")]))),
        // A path that would leave --out.
        (
            vec![declared("../escape", b"bak")],
            Some(stored(&[("../escape", b"bak")])),
        ),
    ] {
        let refused = verified_outputs(&outputs, archive).unwrap_err();
        assert_eq!(refused.code(), "network_result_invalid");
    }
}

#[test]
fn an_existing_output_or_a_case_collision_refuses_before_any_write() {
    let out = tempfile::tempdir().unwrap();
    std::fs::write(out.path().join("b.kmz"), b"kept").unwrap();
    let outputs = vec![declared("a.kmz", b"a"), declared("b.kmz", b"b")];
    let refused = materialize(out.path(), &outputs, vec![b"a".to_vec(), b"b".to_vec()]).unwrap_err();
    assert_eq!(refused.code(), "output_exists");
    assert!(!out.path().join("a.kmz").exists(), "nothing is written first");
    assert_eq!(std::fs::read(out.path().join("b.kmz")).unwrap(), b"kept");

    let outputs = vec![declared("A.kmz", b"a"), declared("a.KMZ", b"b")];
    let refused = materialize(out.path(), &outputs, vec![b"a".to_vec(), b"b".to_vec()]).unwrap_err();
    assert_eq!(refused.code(), "output_path_collision");
}

#[test]
fn the_report_names_each_source_path_by_index() {
    let out = tempfile::tempdir().unwrap();
    let path = write_report(
        out.path(),
        json!({"sources": [{"source_index": 1, "name": "b"}, {"source_index": 0, "name": "a"}]}),
        &["/w/a".into(), "/w/b".into()],
    )
    .unwrap();
    let report: Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    assert_eq!(report["sources"][0]["path"], "/w/b");
    assert_eq!(report["sources"][1]["path"], "/w/a");
}
