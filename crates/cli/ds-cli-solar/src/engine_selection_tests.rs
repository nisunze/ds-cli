//! Actual local consumers over a relocated native-owned fixture selection.
//! Fixture receipts grant no public installation or publication authority.
#![cfg(unix)]
use super::*;
use ds_command_kernel::{module_install::*, module_release::*};
use serde_json::{Value, json};
use std::{fs, path::Path, path::PathBuf};

// Use the existing Solar owner's retained-input fixture; no test preparation IO.
#[path = "../../../../ds-command-kernel/crates/ds-compute-runtime/tests/common/mod.rs"]
mod prepared_fixture;

pub(super) fn select(
    base: &Path,
    lane: &str,
    binary: &Path,
    identity: &Value,
    trailer: &[u8],
) -> PathBuf {
    let mut bytes = fs::read(binary).unwrap();
    bytes.extend_from_slice(trailer);
    let sha256 = ds_solar_contracts::digest::hex_sha256(&bytes);
    let root = base.join("solar").join(lane);
    let version = root.join("versions").join(&sha256);
    fs::create_dir_all(&version).unwrap();
    let executable = version.join("solar");
    fs::write(&executable, &bytes).unwrap();
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
    let source = identity["source_sha"].as_str().unwrap();
    let core_version = identity["version"].as_str().unwrap();
    let release = validate_module_release("solar", lane, &json!({
        "schema":"ds.module-release/v1", "component":"solar", "channel":lane,
        "product_id":format!("rw.datasolutions.module.solar{}",if lane == "canary" { ".canary" } else { "" }),
        "version":format!("{core_version}-r1"),"protocol":ModuleComponent::Solar.protocol(),
        "released_at":"2026-10-14T10:00:00Z","status":"active","assets":[{
            "platform":"linux","arch":"x86_64","kind":"module-executable",
            "protocol":ModuleComponent::Solar.protocol(),
            "url":format!("https://storage.googleapis.com/synthetic-module-fixtures/modules/solar/{lane}/{core_version}-r1/solar"),
            "size_bytes":bytes.len(),"sha256":sha256,"source_sha":source,
            "built_at":"2026-10-14T09:00:00Z","source_revisions":{"ds-solar":source},
            "engines":[{"engine":"ds-solar","release":format!("ds-solar@{core_version}+{source}"),"manifest_sha256":identity["build_manifest_sha256"]}]
        }]
    })).unwrap();
    let active = InstalledModule {
        schema: INSTALLED_SCHEMA.into(),
        authority: ReleaseAuthorityReceipt {
            gateway_origin: "https://synthetic-module-authority.ue.gateway.dev".into(),
            native_catalog_sha256: "a".repeat(64),
            release_sha256: ds_solar_contracts::digest::hex_sha256(
                &serde_json::to_vec(&release).unwrap(),
            ),
        },
        release,
        platform: "linux".into(),
        engine_identity: identity.clone(),
    };
    let selected = ModuleSelection {
        schema: SELECTION_SCHEMA.into(),
        component: ModuleComponent::Solar,
        channel: lane.into(),
        generation: 1,
        active,
        previous: None,
    };
    validate_selection(&selected, ModuleComponent::Solar, lane, "linux").unwrap();
    let next = root.join("selection.next");
    fs::write(&next, serde_json::to_vec(&selected).unwrap()).unwrap();
    fs::rename(next, root.join("selection.json")).unwrap();
    executable
}

#[test]
#[ignore = "requires an explicitly selected qualified Solar executable"]
fn selected_lane_and_captured_consumers_keep_exact_offline_calculation_after_pointer_changes() {
    assert!(std::env::var_os("DS_SOLAR_ENGINE_BIN").is_none());
    let binary = PathBuf::from(
        std::env::var_os("DS_MODULE_TEST_SOLAR_BIN").expect("qualified Solar executable"),
    );
    let identity = SOLAR_ENGINE
        .capture_pinned(&binary)
        .unwrap()
        .call_json("build-info", &[], DISCOVERY_TIMEOUT)
        .unwrap();
    let scratch = tempfile::tempdir().unwrap();
    let base = scratch.path().join("addons");
    let stable = select(&base, "stable", &binary, &identity, b"");
    let first = select(&base, "canary", &binary, &identity, b"");
    let engine_inputs = ds_cli_contract::args::parse(&engine::COMMAND, &[]).unwrap();
    assert_eq!(
        capture_engine_at(&engine_inputs, &base)
            .unwrap()
            .executable(),
        stable
    );
    let canary =
        ds_cli_contract::args::parse(&engine::COMMAND, &["--lane".into(), "canary".into()])
            .unwrap();
    let held_identity = capture_engine_at(&canary, &base).unwrap();
    let request = scratch.path().join("request.json");
    let request_bytes = prepared_fixture::prepared_solar_request();
    fs::write(&request, &request_bytes).unwrap();
    let out = scratch.path().join("out");
    let inputs = ds_cli_contract::args::parse(
        &calculate_prepared::COMMAND,
        &[
            "--request".into(),
            request.to_str().unwrap().into(),
            "--out".into(),
            out.to_str().unwrap().into(),
            "--lane".into(),
            "canary".into(),
        ],
    )
    .unwrap();
    let held_task = capture_engine_at(&inputs, &base).unwrap();
    let second = select(
        &base,
        "canary",
        &binary,
        &identity,
        b"\nfixture-only-ELF-trailer\n",
    );
    assert_ne!(first, second);
    assert_eq!(
        capture_engine_at(&canary, &base).unwrap().executable(),
        second
    );
    let inspected = engine::run_captured(held_identity).unwrap();
    assert_eq!(inspected["path"], first.to_str().unwrap());
    let selected = base.join("solar/canary/selection.json");
    fs::write(&selected, b"{}").unwrap();
    assert_eq!(
        capture_engine_at(&canary, &base).err().unwrap().code(),
        "module_receipt_invalid"
    );
    let completion = calculate_prepared::run_captured(&inputs, held_task).unwrap();
    let expected_identity: ds_solar_contracts::EngineIdentity =
        serde_json::from_value(identity.clone()).unwrap();
    assert_eq!(
        completion["engine"],
        serde_json::to_value(&expected_identity).unwrap()
    );
    assert_eq!(completion["network"], false);
    let receipt: ds_solar_contracts::PreparedCalculationTaskResult =
        serde_json::from_slice(&fs::read(out.join("task-result.json")).unwrap()).unwrap();
    assert_eq!(receipt.engine, expected_identity);
    assert_eq!(completion["result_digest"], receipt.result.result_digest);
    for artifact in receipt.outputs {
        let bytes = fs::read(out.join("artifacts").join(artifact.name)).unwrap();
        assert_eq!(bytes.len() as u64, artifact.byte_count);
        assert_eq!(
            ds_solar_contracts::digest::sha256_hex(&bytes),
            artifact.content_digest
        );
    }
    assert_eq!(fs::read(&request).unwrap(), request_bytes);
    println!(
        "qualified Solar: declared lane, held identity/task path, exact artifacts and corrupt-current refusal verified"
    );
}
