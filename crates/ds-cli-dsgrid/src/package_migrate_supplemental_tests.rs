use super::*;
fn inputs(extra: Vec<String>) -> Inputs {
    let mut tokens = vec![
        "--path".into(),
        "unused.dsgrid".into(),
        "--expected-source-sha256".into(),
        "a".repeat(64),
        "--dry-run".into(),
    ];
    tokens.extend(extra);
    ds_cli_contract::parse(&COMMAND, &tokens).unwrap()
}
#[test]
fn successor_flags_are_complete_and_opt_in_before_file_reads() {
    assert!(successor_inputs(&inputs(vec![])).unwrap().is_none());
    assert!(successor_inputs(&inputs(vec!["--successor".into()])).is_err());
    let flags = [
        "--current-path",
        "current.dsgrid",
        "--expected-current-sha256",
        "current-sha",
        "--migration-receipt",
        "receipt.json",
        "--expected-migration-receipt-sha256",
        "receipt-sha",
    ];
    assert!(successor_inputs(&inputs(flags.iter().map(|v| (*v).into()).collect())).is_err());
    let mut complete = vec!["--successor".into()];
    complete.extend(flags.iter().map(|v| (*v).into()));
    assert_eq!(
        successor_inputs(&inputs(complete)).unwrap(),
        Some((
            "current.dsgrid",
            "current-sha",
            "receipt.json",
            "receipt-sha"
        ))
    );
}

fn controls_dir() -> std::path::PathBuf {
    let scratch = std::env::var_os("TMPDIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../out"));
    std::fs::create_dir_all(&scratch).expect("create disk scratch parent");
    let scratch = std::fs::canonicalize(scratch).expect("existing disk scratch parent");
    assert!(
        scratch.is_absolute() && !scratch.starts_with("/tmp") && !scratch.starts_with("/dev/shm"),
        "migration controls require disk TMPDIR"
    );
    let path = scratch.join("package-migrate-input-controls").join(format!(
        "{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&path).unwrap();
    std::fs::canonicalize(path).unwrap()
}
#[test]
fn source_archive_is_exact_and_receipt_commits_only_after_valid_readback() {
    let root = controls_dir();
    let blank =
        ds_grid_exchange::create_blank_model(&ds_grid_exchange::BlankModelRequest::default())
            .unwrap();
    let output = ds_grid_exchange::package_migration::migrate(&blank.bytes).unwrap();
    let receipt = serde_json::to_value(&output.receipt).unwrap();
    let source = output.receipt.source_sha256.clone();
    let good = root.join("source-archived");
    materialize_preserving_source(&good, &output.bytes, &receipt, &source, Some(&blank.bytes))
        .unwrap();
    assert_eq!(
        std::fs::read(good.join("original.dsgrid")).unwrap(),
        blank.bytes
    );
    assert_eq!(
        std::fs::read(good.join("model.dsgrid")).unwrap(),
        output.bytes
    );
    assert!(good.join("migration.receipt.json").is_file());
    assert!(
        materialize_preserving_source(&good, &output.bytes, &receipt, &source, Some(&blank.bytes))
            .is_err()
    );
    let bad = root.join("wrong-original");
    assert!(
        materialize_preserving_source(
            &bad,
            &output.bytes,
            &receipt,
            &source,
            Some(b"different original")
        )
        .is_err()
    );
    assert!(!bad.join("migration.receipt.json").exists());
    assert!(!bad.join("model.dsgrid").exists());
    assert!(!bad.join("original.dsgrid").exists());
}

fn fixture() -> (std::path::PathBuf, std::path::PathBuf, Value) {
    let out = controls_dir();
    let native = out.join("source.012");
    let bytes = b"exact immutable input bytes";
    std::fs::write(&native, bytes).unwrap();
    let digest = format!("{:x}", Sha256::digest(bytes));
    // This is an input-parser witness, not native execution evidence. The
    // migration separately compares bytes to immutable original row facts.
    let receipt = out.join("extraction.receipt.json");
    let witness = json!({"command":"dsgrid.asset.extract","status":"ok","data":{"package_sha256":"b".repeat(64),"sha256":digest,"byte_len":bytes.len(),"leaf":"source.012","verified":true}});
    let raw = serde_json::to_vec(&witness).unwrap();
    std::fs::write(&receipt, &raw).unwrap();
    let manifest = json!({"schema":"ds.grid.package-migration-supplemental-resources/v1","source_package_sha256":"a".repeat(64),"resources":[{"resource_id":"original-resource","invariant_leaf":"source.012","path":native,"expected_sha256":digest,"byte_len":bytes.len(),"source_package_sha256":"b".repeat(64),"extraction_receipt_path":receipt,"extraction_receipt_sha256":format!("{:x}",Sha256::digest(&raw))}]});
    (out.join("supplemental.json"), receipt, manifest)
}
fn manifest_inputs(path: &Path, manifest: &Value) -> Inputs {
    let bytes = serde_json::to_vec(manifest).unwrap();
    std::fs::write(path, &bytes).unwrap();
    inputs(vec![
        "--supplemental-resources".into(),
        path.to_str().unwrap().into(),
        "--expected-supplemental-sha256".into(),
        format!("{:x}", Sha256::digest(&bytes)),
    ])
}
#[test]
fn paired_flags_and_actual_bounded_read_refuse() {
    assert!(
        load_supplemental(&inputs(vec![]), &"a".repeat(64))
            .unwrap()
            .0
            .is_empty()
    );
    for extra in [
        vec!["--supplemental-resources".into(), "/unused.json".into()],
        vec!["--expected-supplemental-sha256".into(), "a".repeat(64)],
    ] {
        assert!(load_supplemental(&inputs(extra), &"a".repeat(64)).is_err());
    }
    let file = controls_dir().join("bounded.bin");
    std::fs::write(&file, b"12345").unwrap();
    assert!(bounded_supplemental_file(&file, 4).is_err());
    assert_eq!(bounded_supplemental_file(&file, 5).unwrap(), b"12345");
}
#[test]
fn exact_input_parser_positive_source_manifest_receipt_pin_negatives() {
    let (path, receipt, mut manifest) = fixture();
    let parsed = manifest_inputs(&path, &manifest);
    assert_eq!(
        load_supplemental(&parsed, &"a".repeat(64)).unwrap().0.len(),
        1
    );
    assert!(load_supplemental(&parsed, &"c".repeat(64)).is_err());
    let stale = inputs(vec![
        "--supplemental-resources".into(),
        path.to_str().unwrap().into(),
        "--expected-supplemental-sha256".into(),
        "0".repeat(64),
    ]);
    assert!(load_supplemental(&stale, &"a".repeat(64)).is_err());
    std::fs::write(&receipt, b"changed receipt").unwrap();
    assert!(load_supplemental(&parsed, &"a".repeat(64)).is_err());
    let (path, _, good) = fixture();
    manifest = good;
    manifest["resources"][0]["source_package_sha256"] = "c".repeat(64).into();
    assert!(load_supplemental(&manifest_inputs(&path, &manifest), &"a".repeat(64)).is_err());
}
