use ds_solar_reference_host::{FixtureReferenceProvider, ReferencePreparer};
#[test]
fn prepare_accepts_local_cache_and_refuses_http_acquisition_flags() {
    let cli = Cli::try_parse_from([
        "ds-solar", "prepare", "--out", "prepared", "--cache", "cache",
    ])
    .unwrap();
    let Command::Prepare(args) = cli.command else {
        panic!("prepare")
    };
    assert_eq!(args.cache.as_deref(), Some(std::path::Path::new("cache")));
    for flag in [
        "--reference-url",
        "--weather-token",
        "--api-key",
        "--weather-cache",
        "--weather-url",
    ] {
        let error =
            Cli::try_parse_from(["ds-solar", "prepare", "--out", "prepared", flag, "value"])
                .unwrap_err();
        assert_eq!(error.kind(), clap::error::ErrorKind::UnknownArgument);
    }
}

use super::*;

fn governed_snapshot_fixture() -> GovernedSnapshot {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../../ds-solar/fixtures/city/aderm_bere.snapshot.json");
    let mut snapshot: serde_json::Value =
        serde_json::from_slice(&std::fs::read(path).expect("fixture snapshot"))
            .expect("fixture json");
    let city_id = "aderm_bere";
    snapshot["_config"] = serde_json::json!({
        "paths": { "root": "eds_project/project-1/eds_solar/aderm_bere" }
    });
    let input_base = serde_json::json!({
        "_root": snapshot.get("_root").unwrap(),
        "_config": snapshot.get("_config").unwrap(),
        "00_templates": snapshot.get("00_templates").unwrap(),
        "01_city_inputs": snapshot.get("01_city_inputs").unwrap(),
    });
    let fingerprint = ds_solar_contracts::digest::hex_sha256(
        &ds_solar_contracts::digest::canonical_json_bytes(&input_base).unwrap(),
    );
    snapshot["_provenance"] = serde_json::json!({
        "schema": 3,
        "source_kind": "authenticated ds-brain desktop snapshot",
        "root": "eds_project/project-1/eds_solar",
        "context": city_id,
        "demand_noise_seed": ds_solar_contracts::digest::demand_noise_seed(city_id).unwrap(),
        "input_base_fingerprint": fingerprint,
    });
    let snapshot_json =
        String::from_utf8(ds_solar_contracts::digest::canonical_json_bytes(&snapshot).unwrap())
            .unwrap();
    GovernedSnapshot {
        snapshot_sha256: ds_solar_contracts::digest::hex_sha256(snapshot_json.as_bytes()),
        snapshot_bytes: snapshot_json.len(),
        snapshot_json,
        input_base_fingerprint: fingerprint,
        media_downloads: vec![],
        source_document_count: 12,
        firestore_read_time: "2026-08-30T00:00:00Z".to_string(),
        snapshot_receipt_id: "21c655a1-1037-52ce-a601-e9c7e3734d53".to_string(),
        snapshot_receipt_expires_at: "2099-08-30T00:00:00Z".to_string(),
    }
}

#[test]
fn governed_snapshot_becomes_sealed_city_input_and_keeps_publication_authority() {
    let intake = governed_intake(governed_snapshot_fixture()).expect("governed intake");
    assert_eq!(intake.schema_version, "ds-solar.governed-city-intake/v1");
    assert_eq!(intake.city_input.identity.project_id, "project-1");
    assert_eq!(intake.city_input.identity.city_id, "aderm_bere");
    assert_eq!(intake.city_input.identity.display_name, "Bere");
    assert_eq!(intake.city_input.revision.source, "ds-brain");
    assert_eq!(
        intake.city_input.revision.revision.as_deref(),
        Some(intake.authority.input_base_fingerprint.as_str())
    );
    assert!(!intake.city_input.content_digest.is_empty());
    intake.city_input.validate().expect("sealed city input");
    assert_eq!(
        intake.authority.snapshot_receipt_id,
        "21c655a1-1037-52ce-a601-e9c7e3734d53"
    );
}

#[test]
fn governed_snapshot_allows_a_display_name_beyond_identifier_length() {
    let mut governed = governed_snapshot_fixture();
    let mut snapshot: serde_json::Value =
        serde_json::from_str(&governed.snapshot_json).expect("snapshot json");
    let display_name = "Long governed city display name "
        .repeat(8)
        .trim()
        .to_string();
    snapshot["01_city_inputs"]["city"]["city"] = serde_json::json!(display_name);
    let input_base = serde_json::json!({
        "_root": snapshot.get("_root").unwrap(),
        "_config": snapshot.get("_config").unwrap(),
        "00_templates": snapshot.get("00_templates").unwrap(),
        "01_city_inputs": snapshot.get("01_city_inputs").unwrap(),
    });
    let fingerprint = ds_solar_contracts::digest::hex_sha256(
        &ds_solar_contracts::digest::canonical_json_bytes(&input_base).unwrap(),
    );
    snapshot["_provenance"]["input_base_fingerprint"] = serde_json::json!(fingerprint);
    governed.snapshot_json = serde_json::to_string(&snapshot).expect("encode snapshot");
    governed.snapshot_bytes = governed.snapshot_json.len();
    governed.snapshot_sha256 =
        ds_solar_contracts::digest::hex_sha256(governed.snapshot_json.as_bytes());
    governed.input_base_fingerprint = fingerprint;

    let intake = governed_intake(governed).expect("long display name accepted");
    assert!(intake.city_input.identity.display_name.len() > 160);
}

#[test]
fn governed_intake_is_a_direct_preparation_input_and_tampering_is_refused() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let path = temporary.path().join("city.intake.json");
    let intake = governed_intake(governed_snapshot_fixture()).expect("governed intake");
    write_json(&path, &intake).expect("write intake");
    let (loaded, authorities) =
        load_governed_intakes(std::slice::from_ref(&path)).expect("load intake");
    assert_eq!(loaded.len(), 1);
    assert_eq!(loaded[0].identity.city_id, "aderm_bere");
    assert_eq!(
        authorities["aderm_bere"].snapshot_receipt_id,
        intake.authority.snapshot_receipt_id
    );

    let mut tampered = intake;
    tampered.authority.input_base_fingerprint = "f".repeat(64);
    write_json(&path, &tampered).expect("replace test intake");
    // The cause sits under the "validating governed intake" context, so the
    // whole chain is read.
    let error = load_governed_intakes(&[path]).expect_err("authority tampering");
    assert!(format!("{error:#}").contains("disagrees"), "{error:#}");
}

#[test]
fn prepared_publication_claim_binds_native_input_to_server_receipt() {
    let intake = governed_intake(governed_snapshot_fixture()).expect("governed intake");
    let fixture_path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../../ds-solar/fixtures");
    let set = fixtures::FixtureSet::new(fixture_path);
    let provider = FixtureReferenceProvider::new(
        set.all_reference_bundles().expect("fixture bundles"),
        ReferencePin::current(),
    )
    .expect("fixture provider");
    let prepared = ReferencePreparer::new()
        .with_provider(&provider)
        .prepare(intake.city_input.clone(), "2026-08-30T00:00:00Z")
        .expect("prepared input")
        .input;

    let claim =
        prepared_publication_claim(&prepared, &intake.authority).expect("publication claim");
    assert_eq!(claim.project_id, prepared.city.identity.project_id);
    assert_eq!(claim.city_id, prepared.city.identity.city_id);
    assert_eq!(claim.city_content_digest, prepared.city.content_digest);
    assert_eq!(claim.prepared_input_digest, prepared.input_digest);
    assert_eq!(
        claim.source_snapshot_sha256,
        intake.authority.source_snapshot_sha256
    );
    assert_eq!(
        claim.input_base_fingerprint,
        intake.authority.input_base_fingerprint
    );
    assert_eq!(
        claim.snapshot_receipt_id,
        intake.authority.snapshot_receipt_id
    );
    assert_eq!(claim.claim_kind, "server-revalidated-publication-handoff");
}

#[test]
fn governed_snapshot_rejects_tampered_bytes_and_fingerprint() {
    let mut tampered = governed_snapshot_fixture();
    tampered.snapshot_json.push(' ');
    tampered.snapshot_bytes += 1;
    assert!(
        governed_intake(tampered)
            .expect_err("tampered bytes")
            .to_string()
            .contains("snapshot_sha256")
    );

    let mut wrong_fingerprint = governed_snapshot_fixture();
    wrong_fingerprint.input_base_fingerprint = "a".repeat(64);
    let parsed: serde_json::Value = serde_json::from_str(&wrong_fingerprint.snapshot_json).unwrap();
    let mut parsed = parsed;
    parsed["_provenance"]["input_base_fingerprint"] = serde_json::json!("a".repeat(64));
    wrong_fingerprint.snapshot_json = serde_json::to_string(&parsed).unwrap();
    wrong_fingerprint.snapshot_bytes = wrong_fingerprint.snapshot_json.len();
    wrong_fingerprint.snapshot_sha256 =
        ds_solar_contracts::digest::hex_sha256(wrong_fingerprint.snapshot_json.as_bytes());
    assert!(
        governed_intake(wrong_fingerprint)
            .expect_err("forged fingerprint")
            .to_string()
            .contains("does not match its read set")
    );
}

#[test]
fn governed_capture_preserves_go_number_and_html_bytes() {
    let mut governed = governed_snapshot_fixture();
    let mut raw: std::collections::BTreeMap<String, Box<serde_json::value::RawValue>> =
        serde_json::from_str(&governed.snapshot_json).unwrap();
    let mut root: std::collections::BTreeMap<String, Box<serde_json::value::RawValue>> =
        serde_json::from_str(raw["_root"].get()).unwrap();
    // Byte spellings produced by Go encoding/json (not serde_json).
    root.insert("encoding_probe".into(), serde_json::value::RawValue::from_string(
        r#"{"large":1775120732258914048,"number":0.000001,"text":"\u003c\u0026\u003e","tiny":1e-7}"#.into()).unwrap());
    raw.insert(
        "_root".into(),
        serde_json::value::to_raw_value(&root).unwrap(),
    );
    let mut provenance: serde_json::Value =
        serde_json::from_str(raw.remove("_provenance").unwrap().get()).unwrap();
    governed.input_base_fingerprint =
        ds_solar_contracts::digest::hex_sha256(&serde_json::to_vec(&raw).unwrap());
    provenance["input_base_fingerprint"] = serde_json::json!(governed.input_base_fingerprint);
    raw.insert(
        "_provenance".into(),
        serde_json::value::to_raw_value(&provenance).unwrap(),
    );
    governed.snapshot_json = serde_json::to_string(&raw).unwrap();
    governed.snapshot_bytes = governed.snapshot_json.len();
    governed.snapshot_sha256 =
        ds_solar_contracts::digest::hex_sha256(governed.snapshot_json.as_bytes());
    governed_intake(governed).expect("Go byte spelling survives capture");
}

#[test]
fn governed_snapshot_requires_project_scope_and_fresh_server_receipt() {
    let mut global = governed_snapshot_fixture();
    let mut parsed: serde_json::Value = serde_json::from_str(&global.snapshot_json).unwrap();
    parsed["_provenance"]["root"] = serde_json::json!("eds_solar");
    global.snapshot_json = serde_json::to_string(&parsed).unwrap();
    global.snapshot_bytes = global.snapshot_json.len();
    global.snapshot_sha256 =
        ds_solar_contracts::digest::hex_sha256(global.snapshot_json.as_bytes());
    assert!(
        governed_intake(global)
            .expect_err("global root")
            .to_string()
            .contains("not project-scoped")
    );

    let mut expired = governed_snapshot_fixture();
    expired.snapshot_receipt_expires_at = "2020-01-01T00:00:00Z".to_string();
    assert!(
        governed_intake(expired)
            .expect_err("expired receipt")
            .to_string()
            .contains("has expired")
    );
}

#[test]
fn durable_create_new_publishes_complete_bytes_and_refuses_replacement() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let path = temporary.path().join("claim.json");
    write_json_new(&path, &serde_json::json!({"complete": true})).expect("create output");
    assert_eq!(
        std::fs::read_to_string(&path).expect("read output"),
        "{\n  \"complete\": true\n}\n"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&path)
                .expect("output metadata")
                .permissions()
                .mode()
                & 0o077,
            0,
            "governed create-new outputs must not grant group/other access"
        );
    }
    assert!(
        temporary
            .path()
            .read_dir()
            .expect("read directory")
            .all(|entry| !entry
                .expect("directory entry")
                .file_name()
                .to_string_lossy()
                .contains(".stage."))
    );
    assert!(
        write_json_new(&path, &serde_json::json!({"complete": false}))
            .expect_err("replacement refused")
            .to_string()
            .contains("refusing to replace")
    );
    assert_eq!(
        std::fs::read_to_string(&path).expect("read retained output"),
        "{\n  \"complete\": true\n}\n"
    );
}

#[cfg(unix)]
#[test]
fn durable_create_new_refuses_a_dangling_destination_link() {
    use std::os::unix::fs::symlink;

    let temporary = tempfile::tempdir().expect("temporary directory");
    let missing = temporary.path().join("missing.json");
    let path = temporary.path().join("claim.json");
    symlink(&missing, &path).expect("dangling link");

    assert!(
        write_json_new(&path, &serde_json::json!({"complete": true}))
            .expect_err("dangling link refused")
            .to_string()
            .contains("refusing to replace")
    );
    assert!(
        std::fs::symlink_metadata(&path)
            .expect("retained link")
            .file_type()
            .is_symlink()
    );
    assert_eq!(std::fs::read_link(&path).expect("link target"), missing);
    assert!(!missing.exists());
}

#[cfg(unix)]
#[test]
fn governed_output_pair_preflight_refuses_an_existing_claim_before_prepare() {
    use std::os::unix::fs::symlink;

    let temporary = tempfile::tempdir().expect("temporary directory");
    let prepared = temporary.path().join("city.prepared.json");
    let claim = temporary
        .path()
        .join("city.prepared-publication-claim.json");
    symlink(temporary.path().join("missing-claim"), &claim).expect("dangling claim");

    assert!(
        require_create_new_paths([prepared.as_path(), claim.as_path()])
            .expect_err("existing claim refused")
            .to_string()
            .contains("refusing to replace")
    );
    assert!(!prepared.exists());
    assert!(
        std::fs::symlink_metadata(&claim)
            .expect("retained claim link")
            .file_type()
            .is_symlink()
    );
}

#[test]
fn governed_snapshot_reader_allows_server_lane_escaping_above_nine_mib() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let path = temporary.path().join("escaped-response.json");
    let snapshot_json = "\"".repeat(5 * 1024 * 1024);
    let governed = GovernedSnapshot {
        snapshot_sha256: ds_solar_contracts::digest::hex_sha256(snapshot_json.as_bytes()),
        snapshot_bytes: snapshot_json.len(),
        snapshot_json,
        input_base_fingerprint: "a".repeat(64),
        media_downloads: vec![],
        source_document_count: 1,
        firestore_read_time: "2026-08-30T00:00:00Z".to_string(),
        snapshot_receipt_id: "21c655a1-1037-52ce-a601-e9c7e3734d53".to_string(),
        snapshot_receipt_expires_at: "2099-08-30T00:00:00Z".to_string(),
    };
    let encoded = serde_json::to_vec(&governed).expect("encode escaped response");
    assert!(encoded.len() > 9 * 1024 * 1024);
    assert!(encoded.len() < MAX_GOVERNED_SNAPSHOT_RESPONSE_BYTES as usize);
    std::fs::write(&path, encoded).expect("write escaped response");

    let decoded: GovernedSnapshot = read_json_bounded(
        &path,
        MAX_GOVERNED_SNAPSHOT_RESPONSE_BYTES,
        "governed Solar snapshot",
    )
    .expect("outer response within server lane");
    assert_eq!(decoded.snapshot_bytes, 5 * 1024 * 1024);
}

#[test]
fn governed_stdin_decoder_accepts_bounded_data_and_drops_signed_urls() {
    let mut governed = governed_snapshot_fixture();
    governed.media_downloads = vec![serde_json::json!({
        "ref": {"media_id": "not-persisted"},
        "download_url": "https://signed.example.invalid/secret"
    })];
    let encoded = serde_json::to_vec(&governed).expect("encode governed data");
    let decoded: GovernedSnapshot = read_json_stream_bounded(
        std::io::Cursor::new(encoded),
        MAX_GOVERNED_SNAPSHOT_RESPONSE_BYTES,
        "governed Solar snapshot on stdin",
    )
    .expect("bounded stdin data");
    let intake = governed_intake(decoded).expect("governed intake");
    assert_eq!(intake.authority.media_download_count, 1);
    let persisted = serde_json::to_string(&intake).expect("persisted intake");
    assert!(!persisted.contains("signed.example.invalid"));
    assert!(!persisted.contains("download_url"));
    assert!(!persisted.contains("not-persisted"));
}

#[test]
fn governed_stdin_decoder_reads_only_limit_plus_one_and_refuses_overflow() {
    let error = read_json_stream_bounded::<serde_json::Value>(
        std::io::Cursor::new(vec![b' '; 9]),
        8,
        "governed Solar snapshot on stdin",
    )
    .expect_err("stdin overflow refused");
    assert_eq!(
        error.to_string(),
        "governed Solar snapshot on stdin exceeds the 8-byte read bound"
    );
}

fn fixture_input(city: &str) -> SolarCityInput {
    let fixtures = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../../ds-solar/fixtures");
    fixtures::FixtureSet::new(fixtures)
        .city_input(city, "test-project", "solar")
        .expect("fixture input")
}

#[test]
fn canonical_city_input_files_are_loaded_in_city_order() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let mut aderm = fixture_input("aderm_bere");
    let mut beinamar = fixture_input("aderm_beinamar");
    // They are only test identities, so their retained content digests
    // need to follow the intentional identity change below.
    aderm.identity.city_id = "a-city".to_string();
    aderm = aderm.sealed();
    beinamar.identity.city_id = "z-city".to_string();
    beinamar = beinamar.sealed();

    let z_path = temporary.path().join("z.json");
    let a_path = temporary.path().join("a.json");
    write_json(&z_path, &beinamar).expect("write z input");
    write_json(&a_path, &aderm).expect("write a input");

    let inputs = load_city_inputs(&[z_path, a_path]).expect("load canonical inputs");
    assert_eq!(
        inputs
            .iter()
            .map(|input| input.identity.city_id.as_str())
            .collect::<Vec<_>>(),
        ["a-city", "z-city"]
    );
}

#[test]
fn unsealed_city_input_is_refused() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let mut input = fixture_input("aderm_bere");
    input.content_digest.clear();
    let path = temporary.path().join("unsealed.json");
    write_json(&path, &input).expect("write input");

    let error = load_city_inputs(&[path]).expect_err("unsealed input must fail");
    assert!(error.to_string().contains("not a sealed"));
}

#[test]
fn unsafe_city_id_is_refused_before_it_becomes_an_output_path() {
    let error = ensure_safe_city_id("../../outside").expect_err("path traversal must fail");
    assert!(error.to_string().contains("safe single path segment"));
}

#[test]
fn bounded_json_refuses_an_oversized_file_before_deserializing_it() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let path = temporary.path().join("oversized-batch.json");
    let file = File::create(&path).expect("create sparse JSON");
    file.set_len(9).expect("extend sparse JSON");

    let error = read_json_bounded::<serde_json::Value>(&path, 8, "test batch manifest")
        .expect_err("oversized JSON must be refused");
    let message = error.to_string();
    assert!(
        message.contains("exceeding the 8-byte read bound"),
        "{message}"
    );
}

#[test]
fn artifact_verification_refuses_current_disk_bytes_above_its_bound() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let path = temporary.path().join("result.json");
    std::fs::write(&path, b"123456789").expect("write oversized current artifact");
    let artifact = BatchArtifact {
        name: "result.json".to_string(),
        role: ds_solar_contracts::ArtifactRole::Export,
        media_type: "application/json".to_string(),
        byte_count: 4,
        content_digest: ds_solar_contracts::digest::sha256_hex(b"1234"),
        city_id: None,
    };

    let error = verify_batch_artifact(temporary.path(), &artifact, 8)
        .expect_err("current oversized artifact must be refused");
    let message = error.to_string();
    assert!(
        message.contains("9 bytes on disk, exceeding the 8-byte"),
        "{message}"
    );
}

#[test]
fn artifact_verification_refuses_same_size_digest_tampering() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let path = temporary.path().join("portfolio-result.json");
    let original = b"sealed";
    let artifact = BatchArtifact {
        name: "portfolio-result.json".to_string(),
        role: ds_solar_contracts::ArtifactRole::PortfolioResult,
        media_type: "application/json".to_string(),
        byte_count: original.len() as u64,
        content_digest: ds_solar_contracts::digest::sha256_hex(original),
        city_id: None,
    };
    std::fs::write(&path, b"change").expect("write same-size tamper");

    let error = verify_batch_artifact(temporary.path(), &artifact, 64)
        .expect_err("same-size tamper must be refused");
    let message = error.to_string();
    assert!(message.contains("digest"), "{message}");
    assert!(message.contains("does not match"), "{message}");
}
