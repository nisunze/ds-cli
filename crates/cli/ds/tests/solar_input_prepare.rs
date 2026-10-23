//! Public cache-only preparation has no executable, provider or signed-in dependency.
use ds_solar_contracts::{
    CityIdentity, EngineIdentity, RunOptions, SnapshotRevision, SolarCityInput,
};
use ds_solar_host::{
    cache::{DirectoryReferenceCache, ReferenceCache},
    preparation::ReferencePreparer,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    path::{Path, PathBuf},
    process::Command,
};

fn fixture_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../../ds-solar/fixtures")
}
fn intake() -> Value {
    let snapshot: Value = serde_json::from_slice(
        &std::fs::read(fixture_root().join("city/aderm_bere.snapshot.json")).unwrap(),
    )
    .unwrap();
    let city = SolarCityInput::from_city_snapshot(
        CityIdentity {
            project_id: "cache-only-test".into(),
            root: "eds_project/cache-only-test/eds_solar".into(),
            city_id: "aderm_bere".into(),
            display_name: "Bere".into(),
        },
        SnapshotRevision {
            source: "ds-brain".into(),
            captured_at: "2026-08-20T00:00:00Z".into(),
            record_updated_at_ms: None,
            revision: Some("a".repeat(64)),
        },
        RunOptions::default(),
        &snapshot,
    )
    .unwrap();
    json!({"schema_version":"ds-solar.governed-city-intake/v1","city_input":city,
        "authority":{"source_snapshot_sha256":"b".repeat(64),"input_base_fingerprint":"a".repeat(64),
        "snapshot_receipt_id":"11111111-1111-4111-8111-111111111111",
        "snapshot_receipt_expires_at":"2099-01-01T00:00:00Z","firestore_read_time":"2026-08-20T00:00:00Z",
        "snapshot_bytes":10,"source_document_count":1,"media_download_count":0}})
}
fn write_intake(path: &Path, value: &Value) {
    std::fs::write(path, serde_json::to_vec(value).unwrap()).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).unwrap();
    }
}
fn seed_cache(path: &Path, intake: &Value) -> PathBuf {
    let city = serde_json::from_value(intake["city_input"].clone()).unwrap();
    let key = ReferencePreparer::new().reference_key(&city).unwrap();
    let reference = fixture_root().join("reference/aderm_bere");
    let read = |name| std::fs::read(reference.join(name)).unwrap();
    let cache = DirectoryReferenceCache::new(path);
    cache
        .put(
            &key,
            &ds_solar_contracts::BundleBytes {
                weather_json: read("weather.json"),
                reference_unit_json: read("reference-unit.json"),
                reference_unit_parquet: read("reference-unit.parquet"),
                manifest_json: read("manifest.json"),
            },
        )
        .unwrap();
    cache.entry_dir(&key)
}
fn invoke(args: &[&str], dir: &Path, proxy: &str) -> (Value, bool) {
    let missing = dir.join("no-solar-binary");
    let output = Command::new(env!("CARGO_BIN_EXE_ds"))
        .args(args)
        .args(["--output", "json"])
        .env("DS_SOLAR_BIN", &missing)
        .env("DS_SOLAR_ENGINE_BIN", &missing)
        .env("PATH", &missing)
        .env("HOME", dir)
        .env("XDG_CONFIG_HOME", dir.join("config"))
        .env("XDG_DATA_HOME", dir.join("data"))
        .env("HTTP_PROXY", proxy)
        .env("HTTPS_PROXY", proxy)
        .env("ALL_PROXY", proxy)
        .env("NO_PROXY", "")
        .output()
        .unwrap();
    (
        serde_json::from_slice(&output.stdout)
            .unwrap_or_else(|e| panic!("{e}: {}", String::from_utf8_lossy(&output.stderr))),
        output.status.success(),
    )
}
fn prepare(path: &Path, cache: &Path, out: &Path, proxy: &str) -> (Value, bool) {
    invoke(
        &[
            "solar",
            "input",
            "prepare",
            "--intake",
            path.to_str().unwrap(),
            "--cache",
            cache.to_str().unwrap(),
            "--out",
            out.to_str().unwrap(),
        ],
        path.parent().unwrap(),
        proxy,
    )
}
#[test]
fn public_cache_hit_preserves_claim_and_artifacts_with_contract_two_provenance() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("intake.json");
    let cache = root.path().join("cache");
    let out = root.path().join("prepared");
    let input = intake();
    write_intake(&path, &input);
    seed_cache(&cache, &input);
    let (value, success) = prepare(&path, &cache, &out, "http://127.0.0.1:1");
    assert!(success, "{value}");
    assert_eq!(value["contract"], 2);
    let data = &value["data"];
    assert_eq!(data["network"], false);
    assert_eq!(data["cache_mode"], "verified-local-only");
    assert!(data.get("engine").is_none());
    let identity = EngineIdentity::current();
    assert_eq!(
        data["preparation_owner"],
        json!({"name":"ds-solar-project","source_sha":(!identity.source_sha.is_empty()).then_some(identity.source_sha),"contracts_build_manifest_sha256":EngineIdentity::publication_manifest_sha256()})
    );
    assert_eq!(data["artifacts"].as_array().unwrap().len(), 3);
    for (artifact, role) in data["artifacts"].as_array().unwrap().iter().zip([
        "prepared_input",
        "publication_claim",
        "server_submission",
    ]) {
        assert_eq!(artifact["role"], role);
        let file = Path::new(artifact["path"].as_str().unwrap());
        let bytes = std::fs::read(file).unwrap();
        assert_eq!(artifact["bytes"], bytes.len());
        assert_eq!(artifact["sha256"], format!("{:x}", Sha256::digest(&bytes)));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(file).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
    }
    let prepared: Value =
        serde_json::from_slice(&std::fs::read(out.join("aderm_bere.prepared.json")).unwrap())
            .unwrap();
    let claim: Value = serde_json::from_slice(
        &std::fs::read(out.join("aderm_bere.prepared-publication-claim.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(
        claim,
        json!({"schema_version":"ds-solar.prepared-publication-claim/v1","claim_kind":"server-revalidated-publication-handoff",
        "project_id":"cache-only-test","city_id":"aderm_bere","city_content_digest":prepared["city"]["content_digest"],"prepared_input_digest":prepared["input_digest"],
        "source_snapshot_sha256":input["authority"]["source_snapshot_sha256"],"input_base_fingerprint":input["authority"]["input_base_fingerprint"],
        "snapshot_receipt_id":input["authority"]["snapshot_receipt_id"],"snapshot_receipt_expires_at":input["authority"]["snapshot_receipt_expires_at"]})
    );
    let before = std::fs::read(out.join("aderm_bere.prepared-publication-claim.json")).unwrap();
    let (retry, success) = prepare(&path, &cache, &out, "http://127.0.0.1:1");
    assert!(!success);
    assert_eq!(retry["error"]["code"], "solar_prepare_output_exists");
    assert_eq!(
        std::fs::read(out.join("aderm_bere.prepared-publication-claim.json")).unwrap(),
        before
    );
}
#[test]
fn missing_and_tampered_inputs_refuse_without_network_or_executable() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("intake.json");
    let cache = root.path().join("cache");
    std::fs::create_dir(&cache).unwrap();
    let input = intake();
    write_intake(&path, &input);
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let proxy = format!("http://{}", listener.local_addr().unwrap());
    let out = root.path().join("missing-output");
    let (miss, success) = prepare(&path, &cache, &out, &proxy);
    assert!(!success);
    assert_eq!(miss["contract"], 2);
    assert_eq!(miss["error"]["code"], "engine_refused");
    assert_eq!(miss["error"]["detail"]["owner"], "ds-solar-project");
    assert!(!out.exists());
    assert_eq!(std::fs::read_dir(&cache).unwrap().count(), 0);
    let entry = seed_cache(&cache, &input);
    std::fs::write(entry.join("weather.json"), b"tampered").unwrap();
    let (bad, success) = prepare(&path, &cache, &out, &proxy);
    assert!(!success);
    assert_eq!(bad["error"]["code"], "engine_refused");
    assert!(!out.exists());
    assert_eq!(
        std::fs::read(entry.join("weather.json")).unwrap(),
        b"tampered"
    );
    let mut changed = input;
    changed["authority"]["input_base_fingerprint"] = json!("c".repeat(64));
    write_intake(&path, &changed);
    let (bad, success) = prepare(&path, &cache, &out, &proxy);
    assert!(!success);
    assert_eq!(bad["error"]["code"], "engine_refused");
    assert!(!out.exists());
    write_intake(&path, &json!({"schema_version":"invalid"}));
    let (bad, success) = prepare(&path, &cache, &out, &proxy);
    assert!(!success);
    assert_eq!(bad["error"]["code"], "engine_refused");
    assert_eq!(
        listener.accept().unwrap_err().kind(),
        std::io::ErrorKind::WouldBlock
    );
}
#[test]
fn discovery_advertises_contract_two_and_no_engine_requirement() {
    let root = tempfile::tempdir().unwrap();
    let (value, success) = invoke(
        &["capabilities", "solar.input.prepare"],
        root.path(),
        "http://127.0.0.1:1",
    );
    assert!(success, "{value}");
    let command = &value["data"]["command"];
    assert_eq!(command["contract"], 2, "{value}");
    assert!(
        command["output"]
            .as_str()
            .unwrap()
            .contains("preparation_owner")
    );
    assert!(
        !command["refusals"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["code"] == "solar_engine_missing")
    );
}

#[test]
fn mcp_projects_the_same_contract_two_preparation_owner() {
    use std::io::Write;
    use std::process::Stdio;
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("intake.json");
    let cache = root.path().join("cache");
    let out = root.path().join("mcp-prepared");
    let input = intake();
    write_intake(&path, &input);
    seed_cache(&cache, &input);
    let missing = root.path().join("no-solar-binary");
    let chapter = ds_cli_mcp::surface::chapter_tool_name(ds_cli_contract::spec::Chapter::Solar);
    let mut child = Command::new(env!("CARGO_BIN_EXE_ds"))
        .args(["mcp", "serve", "--exposure", "chapters"])
        .env("DS_SOLAR_BIN", &missing)
        .env("DS_SOLAR_ENGINE_BIN", &missing)
        .env("PATH", &missing)
        .env("HOME", root.path())
        .env("XDG_CONFIG_HOME", root.path().join("config"))
        .env("XDG_DATA_HOME", root.path().join("data"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    for request in [
        json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"ds_catalog","arguments":{"command":"solar.input.prepare"}}}),
        json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":chapter,"arguments":{"operation":"describe","command":"solar.input.prepare"}}}),
        json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":chapter,"arguments":{"operation":"invoke","command":"solar.input.prepare","arguments":{"intake":path,"cache":cache,"out":out}}}}),
        json!({"jsonrpc":"2.0","id":999,"method":"shutdown"}),
    ] {
        let stdin = child.stdin.as_mut().unwrap();
        serde_json::to_writer(&mut *stdin, &request).unwrap();
        stdin.write_all(b"\n").unwrap();
    }
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let responses: Vec<Value> = String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    let envelope = |id| {
        responses.iter().find(|r| r["id"] == id).unwrap()["result"]["structuredContent"].clone()
    };
    assert_eq!(envelope(1)["next"]["tool"], chapter);
    assert_eq!(envelope(2)["data"]["command"]["contract"], 2);
    let result = envelope(3);
    assert_eq!(result["contract"], 2, "{result}");
    assert_eq!(
        result["data"]["preparation_owner"]["name"], "ds-solar-project",
        "{result}"
    );
    assert_eq!(result["data"]["artifacts"].as_array().unwrap().len(), 3);
}
