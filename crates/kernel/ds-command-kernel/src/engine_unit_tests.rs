use super::*;
use serde_json::json;

const GUARD: ActivationGuard = ActivationGuard {
    unsaved_edits: false,
    uncaptured_sessions: 0,
    host_restart_required: false,
};

fn catalog(name: &str) -> Value {
    let text = match name {
        "network" => include_str!("module_release/fixtures/network-wasm-canary.json"),
        _ => include_str!("module_release/fixtures/reporter-wasm-canary.json"),
    };
    serde_json::from_str::<Value>(text).unwrap()["release"].clone()
}
fn published_identity(unit: &EngineUnit) -> Value {
    let EngineUnit::Published(unit) = unit else {
        unreachable!()
    };
    let asset = &unit.release.assets[0];
    json!({"schema":"ds.network-engine/v1","protocolMajor":1,"protocolMinor":0,"coreVersion":"0.1.0",
        "sourceSha":asset.source_sha,"dependencyClosureSha256":asset.engines[0].manifest_sha256,
        "environment":"wasm","runtime":{"environment":"wasm","logicalCpus":1,"internalWorkerThreads":1},
        "jsonObjectOrder":"sorted","capabilities":["lv_processing_v1","design_snapshot_v1"]})
}
fn published() -> (InstalledEngineUnit, Vec<VerifiedFile>, Value) {
    let unit = inspect(ModuleComponent::Network, "canary", &catalog("network")).unwrap();
    let verified = unit.files();
    let identity = published_identity(&unit);
    let receipt = prepare(
        unit,
        published_authority("https://gateway.example/", &"a".repeat(64)),
        &verified,
        true,
    )
    .unwrap();
    (receipt, verified, identity)
}
fn local_identity() -> Value {
    json!({"schema":"ds.network-engine/v1","protocolMajor":1,"protocolMinor":0,"coreVersion":"0.1.0",
        "sourceSha":"3".repeat(40),"dependencyClosureSha256":"4".repeat(64),
        "environment":"wasm","runtime":{"environment":"wasm","logicalCpus":1,"internalWorkerThreads":1},
        "jsonObjectOrder":"sorted","capabilities":["lv_processing_v1"]})
}
fn local_files() -> Vec<VerifiedFile> {
    NETWORK_FILES
        .iter()
        .enumerate()
        .map(|(index, name)| VerifiedFile {
            filename: (*name).into(),
            sha256: format!("{index}").repeat(64),
            size_bytes: 1000 + index as i64,
        })
        .collect()
}
fn local() -> InstalledEngineUnit {
    let unit = local_unit(
        ModuleComponent::Network,
        "canary",
        &local_identity(),
        &local_files(),
    )
    .unwrap();
    let authority = local_authority(&unit).unwrap();
    prepare(unit, authority, &local_files(), true).unwrap()
}

#[test]
fn published_unit_installs_captures_and_admits_only_its_producer_identity() {
    let (receipt, verified, identity) = published();
    assert_eq!(
        capture(None, ModuleComponent::Network, "canary")
            .unwrap_err()
            .code,
        "module_not_installed"
    );
    let selected = activate(None, receipt.clone(), 0, true, GUARD).unwrap();
    assert_eq!(selected.generation, 1);
    let captured = capture(Some(&selected), ModuleComponent::Network, "canary").unwrap();
    assert_eq!(captured.receipt, receipt);
    let descriptor =
        capture_byte_descriptor(&captured, ModuleComponent::Network, "canary", UiScope::Release)
            .unwrap();
    assert_eq!(descriptor.unit_sha256, receipt.unit.unit_sha256());
    assert_eq!(descriptor.version, "0.1.0-r1");
    assert_eq!(descriptor.files.len(), 4);
    admit_loaded(&captured, &verified, &identity).unwrap();

    // Corrupt bytes, a crossed identity or a unit without the capability the
    // Kernel calls it for refuse with typed codes; nothing is repaired.
    let mut corrupt = verified.clone();
    corrupt[1].sha256 = "0".repeat(64);
    assert_eq!(
        admit_loaded(&captured, &corrupt, &identity).unwrap_err().code,
        "module_artifact_invalid"
    );
    assert_eq!(
        admit_loaded(&captured, &verified[..3], &identity)
            .unwrap_err()
            .code,
        "module_artifact_invalid"
    );
    for (field, value) in [
        ("sourceSha", json!("0".repeat(40))),
        ("dependencyClosureSha256", json!("0".repeat(64))),
        ("coreVersion", json!("0.2.0")),
        ("environment", json!("native")),
        ("capabilities", json!(["design_snapshot_v1"])),
    ] {
        let mut crossed = identity.clone();
        crossed[field] = value;
        assert_eq!(
            admit_loaded(&captured, &verified, &crossed)
                .unwrap_err()
                .code,
            "module_engine_incompatible",
            "{field}"
        );
    }
    // A newer unit that declares more capabilities is still admitted.
    let mut additive = identity.clone();
    additive["capabilities"] = json!(["lv_processing_v1", "a_future_capability_v9"]);
    additive["aFutureMember"] = json!({"ignored": true});
    admit_loaded(&captured, &verified, &additive).unwrap();

    // A page of another lane or scope never takes this capture.
    assert_eq!(
        capture_byte_descriptor(&captured, ModuleComponent::Network, "canary", UiScope::LocalDev)
            .unwrap_err()
            .code,
        "module_receipt_invalid"
    );
    assert!(capture(Some(&selected), ModuleComponent::Network, "stable").is_err());
    assert!(capture(Some(&selected), ModuleComponent::Reporter, "canary").is_err());
}

#[test]
fn selection_changes_are_generation_fenced_and_keep_the_previous_unit() {
    let (published, _, _) = published();
    let selected = activate(None, published.clone(), 0, true, GUARD).unwrap();
    assert_eq!(
        activate(Some(&selected), published.clone(), 0, true, GUARD)
            .unwrap_err()
            .code,
        "module_selection_changed"
    );
    assert_eq!(
        activate(None, published.clone(), 0, false, GUARD)
            .unwrap_err()
            .code,
        "module_consent_required"
    );
    let held = ActivationGuard {
        uncaptured_sessions: 1,
        ..GUARD
    };
    assert_eq!(
        activate(None, published.clone(), 0, true, held)
            .unwrap_err()
            .code,
        "module_activation_deferred"
    );
    // Re-activating the active unit is idempotent.
    assert_eq!(
        activate(Some(&selected), published.clone(), 1, true, GUARD).unwrap(),
        selected
    );
    let removed = remove(&selected, 1, true).unwrap();
    assert_eq!(removed.generation, 2);
    assert_eq!(
        capture(Some(&removed), ModuleComponent::Network, "canary")
            .unwrap_err()
            .code,
        "module_removed"
    );
    assert_eq!(remove(&removed, 2, true).unwrap_err().code, "module_removed");
    let restored = rollback(&removed, 2, true, GUARD).unwrap();
    assert_eq!(restored.generation, 3);
    assert_eq!(
        capture(Some(&restored), ModuleComponent::Network, "canary")
            .unwrap()
            .receipt,
        published
    );
    assert_eq!(
        rollback(&restored, 3, true, GUARD).unwrap_err().code,
        "module_rollback_unavailable"
    );
}

#[test]
fn local_build_is_canary_local_dev_only_and_admits_exactly_its_tested_identity() {
    let receipt = local();
    assert_eq!(receipt.unit.scope(), UiScope::LocalDev);
    assert_eq!(receipt.unit.version(), format!("local-{}", "3".repeat(40)));
    let selected = activate(None, receipt.clone(), 0, true, GUARD).unwrap();
    let captured = capture(Some(&selected), ModuleComponent::Network, "canary").unwrap();
    let descriptor =
        capture_byte_descriptor(&captured, ModuleComponent::Network, "canary", UiScope::LocalDev)
            .unwrap();
    assert_eq!(descriptor.scope, UiScope::LocalDev);
    admit_loaded(&captured, &local_files(), &local_identity()).unwrap();
    let mut rebuilt = local_identity();
    rebuilt["dependencyClosureSha256"] = json!("5".repeat(64));
    assert_eq!(
        admit_loaded(&captured, &local_files(), &rebuilt)
            .unwrap_err()
            .code,
        "module_engine_incompatible"
    );
    // A release page never loads a local build.
    assert_eq!(
        capture_byte_descriptor(&captured, ModuleComponent::Network, "canary", UiScope::Release)
            .unwrap_err()
            .code,
        "module_receipt_invalid"
    );

    let identity = local_identity();
    assert_eq!(
        local_unit(ModuleComponent::Network, "stable", &identity, &local_files())
            .unwrap_err()
            .code,
        "module_scope_invalid"
    );
    assert_eq!(
        local_unit(ModuleComponent::Network, "canary", &identity, &local_files()[..3])
            .unwrap_err()
            .code,
        "module_artifact_invalid"
    );
    let mut foreign = local_files();
    foreign[0].filename = "ds_reporter_wasm.js".into();
    assert_eq!(
        local_unit(ModuleComponent::Network, "canary", &identity, &foreign)
            .unwrap_err()
            .code,
        "module_artifact_invalid"
    );
    let mut unpinned = identity.clone();
    unpinned["sourceSha"] = Value::Null;
    assert_eq!(
        local_unit(ModuleComponent::Network, "canary", &unpinned, &local_files())
            .unwrap_err()
            .code,
        "module_engine_incompatible"
    );
    assert_eq!(
        local_unit(ModuleComponent::Solar, "canary", &identity, &local_files())
            .unwrap_err()
            .code,
        "module_release_target_unsupported"
    );
    // A local unit cannot borrow a release authority, nor the reverse.
    let unit = local_unit(ModuleComponent::Network, "canary", &identity, &local_files()).unwrap();
    assert_eq!(
        prepare(
            unit,
            published_authority("https://gateway.example/", &"a".repeat(64)),
            &local_files(),
            true
        )
        .unwrap_err()
        .code,
        "module_receipt_invalid"
    );
    // Published and local receipts never share one selection.
    let (published, _, _) = published();
    let selected = activate(None, published, 0, true, GUARD).unwrap();
    assert_eq!(
        activate(Some(&selected), receipt, 1, true, GUARD)
            .unwrap_err()
            .code,
        "module_receipt_invalid"
    );
}

#[test]
fn tampered_receipts_refuse_and_additive_capture_members_are_accepted() {
    let (receipt, verified, identity) = published();
    let selected = activate(None, receipt.clone(), 0, true, GUARD).unwrap();
    let captured = capture(Some(&selected), ModuleComponent::Network, "canary").unwrap();
    let mut wire = serde_json::to_value(&captured).unwrap();
    wire["coreAddedLater"] = json!(1);
    wire["receipt"]["coreAddedLater"] = json!("a newer Core member");
    let decoded: EngineUnitCapture = serde_json::from_value(wire).unwrap();
    admit_loaded(&decoded, &verified, &identity).unwrap();

    let mut tampered = captured.clone();
    let EngineUnit::Published(unit) = &mut tampered.receipt.unit else {
        unreachable!()
    };
    unit.unit_sha256 = "f".repeat(64);
    assert_eq!(
        admit_loaded(&tampered, &verified, &identity)
            .unwrap_err()
            .code,
        "module_receipt_invalid"
    );
    let mut wrong_generation = captured.clone();
    wrong_generation.generation = 0;
    assert_eq!(
        admit_loaded(&wrong_generation, &verified, &identity)
            .unwrap_err()
            .code,
        "module_receipt_invalid"
    );
    let mut withdrawn = catalog("network");
    withdrawn["status"] = json!("blocked");
    let unit = inspect(ModuleComponent::Network, "canary", &withdrawn).unwrap();
    let files = unit.files();
    assert_eq!(
        prepare(
            unit,
            published_authority("https://gateway.example/", &"a".repeat(64)),
            &files,
            true
        )
        .unwrap_err()
        .code,
        "module_release_withdrawn"
    );
    assert_eq!(
        inspect(ModuleComponent::Network, "canary", &catalog("reporter"))
            .unwrap_err()
            .code,
        "module_release_identity_mismatch"
    );
    let unit = inspect(ModuleComponent::Network, "canary", &catalog("network")).unwrap();
    for origin in ["http://gateway.example/", "https://gateway.example/path", "not a url"] {
        assert_eq!(
            prepare(
                unit.clone(),
                published_authority(origin, &"a".repeat(64)),
                &verified,
                true
            )
            .unwrap_err()
            .code,
            "module_receipt_invalid",
            "{origin}"
        );
    }
}

#[test]
fn each_build_declares_one_host_effect_with_its_own_lane_and_scope() {
    let desktop = host_for(ModuleComponent::Network, "canary", false, true).unwrap();
    assert_eq!(desktop.host, "core");
    assert_eq!(desktop.scope, UiScope::Release);
    let capture = desktop.capture.unwrap();
    assert_eq!(capture.command, "core.component.capture");
    assert_eq!(
        capture.args,
        json!({"component":"network","channel":"canary","runtime":"wasm","scope":"release"})
    );
    assert_eq!(desktop.member.unwrap().command, "core.component.member");
    let stable = host_for(ModuleComponent::Network, "stable", false, true).unwrap();
    assert_eq!(stable.capture.unwrap().args["channel"], "stable");
    for (core_attached, door) in [(true, "core"), (false, "development")] {
        let local = host_for(ModuleComponent::Network, "canary", true, core_attached).unwrap();
        assert_eq!(local.host, door);
        assert_eq!(local.scope, UiScope::LocalDev);
        assert_eq!(local.capture.unwrap().args["scope"], "local-dev");
    }
    let hosted = host_for(ModuleComponent::Network, "stable", false, false).unwrap();
    assert_eq!(hosted.host, "browser");
    assert!(hosted.capture.is_none() && hosted.member.is_none());
    assert_eq!(
        host_for(ModuleComponent::Network, "stable", true, true)
            .unwrap_err()
            .code,
        "module_scope_invalid"
    );
    assert_eq!(
        host_for(ModuleComponent::Solar, "canary", false, true)
            .unwrap_err()
            .code,
        "module_release_target_unsupported"
    );
}

#[test]
fn page_requests_are_closed_and_bounded() {
    for input in [
        &b""[..],
        b"{",
        br#"{"kind":"unknown","component":"network"}"#,
        br#"{"kind":"host","component":"network","extra":1}"#,
        br#"{"kind":"host","component":"solarium"}"#,
    ] {
        assert_eq!(
            evaluate(input).unwrap_err().code,
            "module_request_invalid",
            "{}",
            String::from_utf8_lossy(input)
        );
    }
    let oversized = vec![b' '; MAX_WIRE_BYTES + 1];
    assert_eq!(
        evaluate(&oversized).unwrap_err().code,
        "module_request_invalid"
    );
    assert_eq!(
        admit_target("network", "canary").unwrap(),
        ModuleComponent::Network
    );
    assert_eq!(
        admit_target("solar", "canary").unwrap_err().code,
        "module_release_target_unsupported"
    );
    assert_eq!(
        admit_target("network", "beta").unwrap_err().code,
        "module_release_channel_invalid"
    );
    assert_eq!(
        member_names(ModuleComponent::Reporter).unwrap()[1],
        "ds_reporter_wasm_bg.wasm"
    );
    for (component, names) in [
        (ModuleComponent::Network, NETWORK_FILES),
        (ModuleComponent::Reporter, REPORTER_FILES),
    ] {
        let stem = crate::module_engine::browser_engine_profile(component)
            .unwrap()
            .stem;
        assert_eq!(
            names,
            [
                format!("{stem}.js"),
                format!("{stem}_bg.wasm"),
                format!("{stem}.d.ts"),
                format!("{stem}_bg.wasm.d.ts")
            ]
        );
    }
}

#[test]
fn the_local_dev_development_host_reads_only_this_builds_capture_and_fixed_members() {
    let development = host_for(ModuleComponent::Network, "canary", true, false).unwrap();
    let desktop = host_for(ModuleComponent::Network, "canary", true, true).unwrap();
    assert_eq!(
        development_reads(&desktop, "http://127.0.0.1:5173")
            .unwrap_err()
            .code,
        "module_host_unavailable"
    );
    for origin in ["file:///tmp", "http://127.0.0.1:5173/sub", "not an origin"] {
        assert_eq!(
            development_reads(&development, origin).unwrap_err().code,
            "module_host_unavailable",
            "{origin}"
        );
    }
    let base = development_reads(&development, "http://127.0.0.1:5173").unwrap();
    assert_eq!(base, "http://127.0.0.1:5173/__ds_local/engine-units/network");

    // Core's refusal keeps Core's own code; an unnamed one does not pass.
    let refusal = br#"{"v":1,"command":"core.component.capture","status":"error","error":{"code":"module_not_installed","message":"no retained engine unit installation"}}"#;
    assert_eq!(
        development_capture(&development, refusal).unwrap_err(),
        (
            "module_not_installed".to_owned(),
            "no retained engine unit installation".to_owned()
        )
    );
    for answer in [
        &b"<html>"[..],
        br#"{"status":"error","error":{"code":"Not A Code","message":"x"}}"#,
        br#"{"status":"pending"}"#,
    ] {
        assert_eq!(
            development_capture(&development, answer).unwrap_err().0,
            "module_host_unavailable"
        );
    }
    // A local-dev capture is admitted with its byte facts, additive members included.
    let selected = activate(None, local(), 0, true, GUARD).unwrap();
    let captured = capture(Some(&selected), ModuleComponent::Network, "canary").unwrap();
    let mut data = serde_json::to_value(&captured).unwrap();
    data["coreAddedLater"] = json!(true);
    let envelope = json!({"v":1,"command":"core.component.capture","status":"ok","data":data,"meta":{"added":1}});
    let admitted =
        development_capture(&development, &serde_json::to_vec(&envelope).unwrap()).unwrap();
    assert_eq!(
        admitted["descriptor"]["unit_sha256"],
        captured.receipt.unit.unit_sha256()
    );
    assert_eq!(
        serde_json::from_value::<EngineUnitCapture>(admitted["capture"].clone()).unwrap(),
        captured
    );
    // A published (release-scope) capture is not this local page's unit.
    let (published, _, _) = published();
    let released = activate(None, published, 0, true, GUARD).unwrap();
    let release_capture = capture(Some(&released), ModuleComponent::Network, "canary").unwrap();
    let envelope = json!({"status":"ok","data":release_capture});
    assert_eq!(
        development_capture(&development, &serde_json::to_vec(&envelope).unwrap())
            .unwrap_err()
            .0,
        "module_receipt_invalid"
    );
    let file = &captured.receipt.unit.files()[0];
    assert_eq!(
        development_member(
            &development,
            &base,
            captured.receipt.unit.unit_sha256(),
            &file.filename,
            file.size_bytes as f64,
            &file.sha256
        )
        .unwrap(),
        (
            format!(
                "{base}/{}/{}",
                captured.receipt.unit.unit_sha256(),
                file.filename
            ),
            file.size_bytes as u64
        )
    );
    for (name, size, digest) in [
        ("../selection.json", 10.0, "a".repeat(64)),
        ("ds_reporter_wasm.js", 10.0, "a".repeat(64)),
        ("ds_network_wasm.js", 0.0, "a".repeat(64)),
        ("ds_network_wasm.js", 1.5, "a".repeat(64)),
        ("ds_network_wasm.js", f64::NAN, "a".repeat(64)),
        ("ds_network_wasm.js", 10.0, "A".repeat(64)),
    ] {
        assert_eq!(
            development_member(&development, &base, &"b".repeat(64), name, size, &digest)
                .unwrap_err()
                .code,
            "module_artifact_invalid",
            "{name} {size}"
        );
    }
}
