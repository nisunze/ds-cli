//! Public cache-only preparation: Core admits the governed intake and reads
//! the verified local cache; the selected Solar addon plans the reference and
//! prepares the input through its typed `prepare_reference` task. No provider,
//! network or signed-in dependency is reached.
use ds_cli_exec::solar::Solar;
use ds_solar_contracts::{
    BundleBytes, CityIdentity, EngineIdentity, PreparedSolarCityInput, ReferencePin, RunOptions,
    SnapshotRevision, SolarCityInput, SolarError,
};
use ds_solar_host::{
    cache::{DirectoryReferenceCache, ReferenceCache},
    preparation::{AddonPreparation, PreparationTasks},
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::OnceLock,
};

const CITY: &str = "aderm_kyabe";

fn fixture_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../../ds-solar/fixtures")
}

/// The exact `solar` addon under test. An explicit `DS_SOLAR_ENGINE_BIN`
/// selects a prebuilt addon; otherwise Cargo builds it from the sibling
/// ds-solar workspace, so the suite never drives a stale executable.
fn engine() -> &'static Path {
    static ENGINE: OnceLock<PathBuf> = OnceLock::new();
    ENGINE.get_or_init(|| {
        if let Some(path) = std::env::var_os("DS_SOLAR_ENGINE_BIN") {
            return std::fs::canonicalize(path).unwrap();
        }
        let output = Command::new(env!("CARGO"))
            .args(["build", "--package", "ds-solar-addon", "--bin", "solar"])
            .arg("--message-format=json-render-diagnostics")
            .arg("--manifest-path")
            .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../../ds-solar/Cargo.toml"))
            .stdin(Stdio::null())
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "building the solar addon failed:\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
        output
            .stdout
            .split(|byte| *byte == b'\n')
            .filter_map(|line| serde_json::from_slice::<Value>(line).ok())
            .filter(|message| {
                message["reason"] == "compiler-artifact"
                    && message["target"]["name"] == "solar"
                    && message["target"]["kind"] == json!(["bin"])
            })
            .find_map(|message| message["executable"].as_str().map(PathBuf::from))
            .expect("cargo reported the solar executable")
    })
}

/// The addon's own preparation capability, the same one the command selects.
fn tasks(scratch: &Path) -> AddonPreparation {
    let engine = engine().to_owned();
    AddonPreparation::from_factory(scratch, move || {
        Solar::capture_pinned(&engine)
            .map_err(|error| SolarError::artifact_failed(error.to_string()))
    })
}

fn intake() -> Value {
    let snapshot: Value = serde_json::from_slice(
        &std::fs::read(fixture_root().join(format!("city/{CITY}.snapshot.json"))).unwrap(),
    )
    .unwrap();
    let city = SolarCityInput::from_city_snapshot(
        CityIdentity {
            project_id: "cache-only-test".into(),
            root: "eds_project/cache-only-test/eds_solar".into(),
            city_id: CITY.into(),
            display_name: "Kyabe".into(),
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

fn bundle() -> BundleBytes {
    let reference = fixture_root().join(format!("reference/{CITY}"));
    let read = |name| std::fs::read(reference.join(name)).unwrap();
    BundleBytes {
        weather_json: read("weather.json"),
        reference_unit_json: read("reference-unit.json"),
        reference_unit_parquet: read("reference-unit.parquet"),
        manifest_json: read("manifest.json"),
    }
}

/// The addon plans the exact reference key; the test stores the sealed bundle
/// through the same verified cache owner the command reads.
fn seed_cache(path: &Path, intake: &Value) -> PathBuf {
    let city = serde_json::from_value(intake["city_input"].clone()).unwrap();
    let scratch = tempfile::tempdir().unwrap();
    let key = tasks(scratch.path())
        .plan(&city, ReferencePin::current())
        .unwrap()
        .key;
    let cache = DirectoryReferenceCache::new(path);
    cache.put(&key, &bundle()).unwrap();
    cache.entry_dir(&key)
}

fn invoke_with(args: &[&str], dir: &Path, proxy: &str, addon: &Path) -> (Value, bool) {
    let output = Command::new(env!("CARGO_BIN_EXE_ds"))
        .args(args)
        .args(["--output", "json"])
        .env("DS_SOLAR_ENGINE_BIN", addon)
        .env_remove("DS_SOLAR_BIN")
        .env("DS_SRE_REPORTING", "off")
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

fn prepare_with(path: &Path, cache: &Path, out: &Path, proxy: &str, addon: &Path) -> (Value, bool) {
    invoke_with(
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
        addon,
    )
}

fn prepare(path: &Path, cache: &Path, out: &Path, proxy: &str) -> (Value, bool) {
    prepare_with(path, cache, out, proxy, engine())
}

/// The committed corpus identity of the sealed Kyabe bundle.
fn corpus_index() -> Value {
    let index: Value = serde_json::from_slice(
        &std::fs::read(fixture_root().join("reference/INDEX.json")).unwrap(),
    )
    .unwrap();
    index["cities"][CITY].clone()
}

#[test]
fn public_cache_hit_preserves_claim_artifacts_and_kyabe_prepared_identity() {
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
    let prepared_bytes = std::fs::read(out.join(format!("{CITY}.prepared.json"))).unwrap();
    let prepared: Value = serde_json::from_slice(&prepared_bytes).unwrap();

    // Kyabe parity: the command's prepared input carries exactly the frozen
    // corpus bundle and weather identity, its digest recomputes, and it equals
    // the addon's own direct preparation of the same city and bundle.
    let typed: PreparedSolarCityInput = serde_json::from_slice(&prepared_bytes).unwrap();
    let corpus = corpus_index();
    assert_eq!(
        typed.reference_unit.manifest.bundle_digest,
        corpus["bundle_digest"]
    );
    assert_eq!(
        typed.reference_unit.receipt.source_key,
        corpus["source_key"]
    );
    assert_eq!(
        typed.weather.content_digest,
        corpus["weather_content_digest"]
    );
    assert_eq!(typed.compute_input_digest().unwrap(), typed.input_digest);
    let city: SolarCityInput = serde_json::from_value(input["city_input"].clone()).unwrap();
    let scratch = tempfile::tempdir().unwrap();
    let direct = tasks(scratch.path())
        .prepare(
            city,
            ReferencePin::current(),
            &bundle(),
            "2026-08-20T00:00:00Z",
        )
        .unwrap();
    assert_eq!(direct.input_digest, typed.input_digest);
    assert_eq!(direct.city, typed.city);
    assert_eq!(direct.weather_key, typed.weather_key);

    let claim: Value = serde_json::from_slice(
        &std::fs::read(out.join(format!("{CITY}.prepared-publication-claim.json"))).unwrap(),
    )
    .unwrap();
    assert_eq!(
        claim,
        json!({"schema_version":"ds-solar.prepared-publication-claim/v1","claim_kind":"server-revalidated-publication-handoff",
        "project_id":"cache-only-test","city_id":CITY,"city_content_digest":prepared["city"]["content_digest"],"prepared_input_digest":prepared["input_digest"],
        "source_snapshot_sha256":input["authority"]["source_snapshot_sha256"],"input_base_fingerprint":input["authority"]["input_base_fingerprint"],
        "snapshot_receipt_id":input["authority"]["snapshot_receipt_id"],"snapshot_receipt_expires_at":input["authority"]["snapshot_receipt_expires_at"]})
    );
    let before =
        std::fs::read(out.join(format!("{CITY}.prepared-publication-claim.json"))).unwrap();
    let (retry, success) = prepare(&path, &cache, &out, "http://127.0.0.1:1");
    assert!(!success);
    assert_eq!(retry["error"]["code"], "solar_prepare_output_exists");
    assert_eq!(
        std::fs::read(out.join(format!("{CITY}.prepared-publication-claim.json"))).unwrap(),
        before
    );
}

#[test]
fn missing_and_tampered_inputs_refuse_without_network() {
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

/// An addon released before preparation moved declares no `prepare_reference`
/// task: Core refuses with the typed update code before any task process and
/// writes nothing.
#[cfg(unix)]
#[test]
fn old_addon_without_preparation_task_refuses_update_required() {
    use std::os::unix::fs::PermissionsExt;
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("intake.json");
    let cache = root.path().join("cache");
    let out = root.path().join("prepared");
    let input = intake();
    write_intake(&path, &input);
    seed_cache(&cache, &input);
    let old = root.path().join("old-solar");
    let identity =
        json!({"name":"ds-solar-engine","schemas":["ds-solar.prepared-calculation-result/v1"]});
    std::fs::write(
        &old,
        format!(
            "#!/bin/sh\nprintf '%s\\n' \"$1\" >> \"$0.calls\"\ncase \"$1\" in\nbuild-info) printf '%s\\n' '{identity}' ;;\n*) exit 91 ;;\nesac\n"
        ),
    )
    .unwrap();
    std::fs::set_permissions(&old, std::fs::Permissions::from_mode(0o700)).unwrap();
    let (value, success) = prepare_with(&path, &cache, &out, "http://127.0.0.1:1", &old);
    assert!(!success, "{value}");
    assert_eq!(
        value["error"]["code"], "solar_engine_update_required",
        "{value}"
    );
    assert!(!out.exists());
    let calls = std::fs::read_to_string(old.with_extension("calls")).unwrap();
    assert!(
        calls.lines().all(|call| call == "build-info"),
        "an undeclared task must not start: {calls}"
    );
}

#[test]
fn discovery_advertises_contract_two_and_the_update_refusal() {
    let root = tempfile::tempdir().unwrap();
    let missing = root.path().join("no-solar-binary");
    let (value, success) = invoke_with(
        &["capabilities", "solar.input.prepare"],
        root.path(),
        "http://127.0.0.1:1",
        &missing,
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
    let refusals = command["refusals"].as_array().unwrap();
    assert!(
        refusals
            .iter()
            .any(|r| r["code"] == "solar_engine_update_required")
    );
    assert!(!refusals.iter().any(|r| r["code"] == "solar_engine_missing"));
}

#[test]
fn mcp_projects_the_same_contract_two_preparation_owner() {
    use std::io::Write;
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("intake.json");
    let cache = root.path().join("cache");
    let out = root.path().join("mcp-prepared");
    let input = intake();
    write_intake(&path, &input);
    seed_cache(&cache, &input);
    let chapter = ds_cli_mcp::surface::chapter_tool_name(ds_cli_contract::spec::Chapter::Solar);
    let mut child = Command::new(env!("CARGO_BIN_EXE_ds"))
        .args(["mcp", "serve", "--exposure", "chapters"])
        .env("DS_SOLAR_ENGINE_BIN", engine())
        .env_remove("DS_SOLAR_BIN")
        .env("DS_SRE_REPORTING", "off")
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
    // MCP and CLI prepare the same Kyabe identity from the same cache.
    let prepared: Value =
        serde_json::from_slice(&std::fs::read(out.join(format!("{CITY}.prepared.json"))).unwrap())
            .unwrap();
    let cli_out = root.path().join("cli-prepared");
    let (cli, success) = prepare(&path, &cache, &cli_out, "http://127.0.0.1:1");
    assert!(success, "{cli}");
    let cli_prepared: Value = serde_json::from_slice(
        &std::fs::read(cli_out.join(format!("{CITY}.prepared.json"))).unwrap(),
    )
    .unwrap();
    assert_eq!(prepared["input_digest"], cli_prepared["input_digest"]);
}
