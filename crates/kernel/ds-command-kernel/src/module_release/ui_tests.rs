use super::*;
use crate::module_engine::valid_compiler_identity;
use serde_json::json;

#[test]
fn capture_byte_descriptor_projects_both_provenances_through_one_serde_contract() {
    let (published, verified, identity) = ready();
    let source = identity["source_sha"].as_str().unwrap();
    let local = prepare_local(
        local_unit(UiComponent::Interactions, "canary", source, &identity, &verified).unwrap(),
        source, &verified, true,
    ).unwrap();
    for receipt in [published, local] {
        let selected = activate(None, receipt, 0, true, guard()).unwrap();
        let captured = capture(Some(&selected), UiComponent::Interactions, "canary", "https://gateway.example").unwrap();
        let descriptor = capture_byte_descriptor(&captured).unwrap();
        assert_eq!(descriptor.schema, CAPTURE_BYTES_SCHEMA);
        assert_eq!(descriptor.component, UiComponent::Interactions);
        assert_eq!(descriptor.channel, "canary");
        assert_eq!(descriptor.scope, captured.receipt.unit.scope());
        assert_eq!(descriptor.unit_sha256, captured.receipt.unit.unit_sha256());
        assert_eq!(descriptor.generation, 1);
        let mut actual = descriptor.files.clone();
        actual.sort_by(|a, b| a.filename.cmp(&b.filename));
        let mut expected = verified.clone();
        expected.sort_by(|a, b| a.filename.cmp(&b.filename));
        assert_eq!(actual, expected);
        let wire = serde_json::to_value(&descriptor).unwrap();
        assert!(wire.get("receipt").is_none() && wire.get("release").is_none());
        assert_eq!(wire.as_object().unwrap().len(), 7);
        assert_eq!(serde_json::from_value::<UiCaptureByteDescriptor>(wire).unwrap(), descriptor);
    }
}

#[test]
fn capture_byte_descriptor_refuses_invalid_generation_unit_and_identity() {
    let (receipt, _, _) = ready();
    let captured = UiCapture { generation: 1, receipt };
    for generation in [0, MAX_GENERATION + 1] {
        let mut invalid = captured.clone();
        invalid.generation = generation;
        assert_eq!(capture_byte_descriptor(&invalid).unwrap_err().code, "module_receipt_invalid");
    }
    let mut invalid = captured.clone();
    if let UiUnit::Published { unit_sha256, .. } = &mut invalid.receipt.unit {
        *unit_sha256 = "f".repeat(64);
    }
    assert!(capture_byte_descriptor(&invalid).is_err());
    let mut invalid = captured;
    invalid.receipt.engine_identity.source_sha = "f".repeat(40);
    assert!(capture_byte_descriptor(&invalid).is_err());
}

#[test]
fn published_wire_is_unchanged_and_matching_numeric_contract_additions_are_compatible() {
    let (catalog, _) = fixture(UiComponent::CommandKernel);
    let original = inspect("command-kernel", "canary", &catalog).unwrap();
    let wire = serde_json::to_value(&original).unwrap();
    assert_eq!(
        wire,
        json!({"release":catalog,"unit_sha256":original.unit_sha256(),"total_bytes":original.total_bytes()})
    );
    let mut compatible = catalog.clone();
    compatible["build_manifest"]["ui_contract"] = json!(2);
    // Numeric metadata can change, but the producer digest must still bind it.
    assert!(inspect("command-kernel", "canary", &compatible).is_err());
    let manifest = parse_identity(&compatible["build_manifest"]).unwrap();
    for asset in compatible["assets"].as_array_mut().unwrap() {
        asset["engines"][0]["manifest_sha256"] = json!(manifest.manifest_sha256());
    }
    let admitted = inspect("command-kernel", "canary", &compatible).unwrap();
    validate_identity(&admitted, &manifest).unwrap();
    assert_ne!(admitted.unit_sha256(), original.unit_sha256());
}

#[test]
fn local_build_roundtrip_has_truthful_provenance_and_cannot_enter_release_staging() {
    for component in [UiComponent::Interactions, UiComponent::CommandKernel] {
        let (catalog, mut identity) = fixture(component);
        identity["ui_contract"] = json!(2);
        let source = identity["source_sha"].as_str().unwrap();
        let published = inspect(component.as_str(), "canary", &catalog).unwrap();
        let verified = files(&published);
        let unit = local_unit(component, "canary", source, &identity, &verified).unwrap();
        let receipt = prepare_local(unit.clone(), source, &verified, true).unwrap();
        let wire = serde_json::to_value(&receipt).unwrap();
        assert!(wire["unit"].get("release").is_none());
        assert!(wire["authority"].get("gateway_origin").is_none());
        assert_eq!(wire["unit"]["local_build"]["scope"], "local-dev");
        let decoded: InstalledUiModule = serde_json::from_value(wire).unwrap();
        assert_eq!(decoded, receipt);
        let selected = activate(None, decoded, 0, true, guard()).unwrap();
        let captured = capture(Some(&selected), component, "canary", "").unwrap();
        admit_loaded(&captured, &verified, &identity).unwrap();
        assert!(acquisition_plan(&unit, "canary", unit.unit_sha256(), true).is_err());
        assert!(prepare(unit.clone(), receipt.authority.clone(), &verified, true).is_err());
        assert!(prepare_local(published, source, &verified, true).is_err());
        assert!(prepare_local(unit.clone(), source, &verified, false).is_err());
        assert!(local_unit(component, "stable", source, &identity, &verified).is_err());
        assert!(local_unit(component, "canary", &"f".repeat(40), &identity, &verified).is_err());
        let mut corrupt = verified.clone();
        corrupt[0].sha256 = "f".repeat(64);
        assert!(admit_loaded(&captured, &corrupt, &identity).is_err());
        let mut wrong_identity = identity.clone();
        wrong_identity["source_sha"] = json!("f".repeat(40));
        assert!(admit_loaded(&captured, &verified, &wrong_identity).is_err());
        let mut duplicate = verified.clone();
        duplicate[3] = duplicate[0].clone();
        assert!(local_unit(component, "canary", source, &identity, &duplicate).is_err());
        let mut forged = receipt.clone();
        forged.authority = UiAuthority::Published(PublishedUiAuthority {
            gateway_origin: "https://gateway.example".into(),
            route_manifest_sha256: "e".repeat(64),
        });
        assert!(admit_retained(&forged, "canary", "https://gateway.example").is_err());
    }
}

fn fixture(component: UiComponent) -> (Value, Value) {
    let (release, identity) = match component {
        UiComponent::Interactions => (
            include_str!("fixtures/interactions-ui-wasm-canary.json"),
            include_str!("fixtures/interactions-ui-engine-build.json"),
        ),
        UiComponent::CommandKernel => (
            include_str!("fixtures/command-kernel-ui-wasm-canary.json"),
            include_str!("fixtures/command-kernel-ui-engine-build.json"),
        ),
    };
    (
        serde_json::from_str(release).unwrap(),
        serde_json::from_str(identity).unwrap(),
    )
}
fn files(unit: &UiUnit) -> Vec<VerifiedFile> {
    unit.published().unwrap()
        .assets
        .iter()
        .map(|asset| {
            let name = filename(asset).unwrap();
            let bytes = format!("fixture:{name}\n");
            VerifiedFile {
                filename: name,
                sha256: format!("{:x}", Sha256::digest(bytes.as_bytes())),
                size_bytes: bytes.len() as i64,
            }
        })
        .collect()
}
fn ready() -> (InstalledUiModule, Vec<VerifiedFile>, Value) {
    let (release, identity) = fixture(UiComponent::Interactions);
    let unit = inspect("interactions", "canary", &release).unwrap();
    let verified = files(&unit);
    let receipt = prepare(
        unit,
        UiAuthority::Published(PublishedUiAuthority {
            gateway_origin: "https://gateway.example".into(),
            route_manifest_sha256: "e".repeat(64),
        }),
        &verified,
        true,
    )
    .unwrap();
    (receipt, verified, identity)
}
fn guard() -> ActivationGuard {
    ActivationGuard {
        unsaved_edits: false,
        uncaptured_sessions: 0,
        host_restart_required: false,
    }
}

#[test]
fn both_fixed_ui_units_preserve_exact_receipts_and_canonical_identity() {
    for component in [UiComponent::Interactions, UiComponent::CommandKernel] {
        let (release, identity) = fixture(component);
        let unit = inspect(component.as_str(), "canary", &release).unwrap();
        assert_eq!(serde_json::to_value(&unit.published().unwrap()).unwrap(), release);
        verify_files(&unit, &files(&unit)).unwrap();
        let identity = parse_identity(&identity).unwrap();
        validate_identity(&unit, &identity).unwrap();
        assert_eq!(
            identity.manifest_sha256(),
            unit.published().unwrap().assets[0].engines[0].manifest_sha256
        );
        let mut reordered = release.clone();
        reordered["assets"].as_array_mut().unwrap().reverse();
        assert_eq!(
            inspect(component.as_str(), "canary", &reordered)
                .unwrap()
                .unit_sha256(),
            unit.unit_sha256()
        );
    }
    assert!(super::super::ModuleComponent::parse("interactions").is_err());
    assert!(admit_target("network", "canary").is_err());
}

#[test]
fn compiler_facts_are_complete_bounded_and_consistent() {
    let (_, identity) = fixture(UiComponent::Interactions);
    let compiler = identity["rustc"].as_str().unwrap();
    assert!(valid_compiler_identity(compiler));
    for invalid in [
        compiler.split("; ").next().unwrap().to_owned(),
        compiler.replace("; host: x86_64-unknown-linux-gnu", ""),
        format!("{compiler}; binary: rustc"),
        format!("{compiler}; unknown: fact"),
        compiler.replace("binary: rustc", "binary: clang"),
        compiler.replace("commit-hash: 1159", "commit-hash: 2159"),
        compiler.replace("commit-date: 2025-09-14", "commit-date: 2025-09-15"),
        compiler.replace("release: 1.90.0", "release: 1.91.0"),
        compiler.replace("LLVM version: 20.1.8", "LLVM version: unknown"),
        compiler.replace(
            "host: x86_64-unknown-linux-gnu",
            "host: x86_64\n-unknown-linux-gnu",
        ),
        "x".repeat(4097),
    ] {
        assert!(!valid_compiler_identity(&invalid), "{invalid}");
    }
}

#[test]
fn closed_ui_identity_rejects_unknown_abi_source_compiler_and_config() {
    let (receipt, _, identity) = ready();
    let mut unknown = identity.clone();
    unknown["future"] = json!(true);
    assert!(parse_identity(&unknown).is_err());
    for (key, value) in [
        ("schema", json!("ds.engine-build/v1")),
        ("component", json!("command-kernel")),
        ("protocol", json!("ds.ui-wasm/v2")),
        ("ui_contract", json!(2)),
        ("source_sha", json!("B".repeat(40))),
        ("source_sha", json!("a".repeat(40))),
        ("cargo_lock_sha256", json!("a".repeat(64))),
        ("rustc", json!("rustc different")),
        ("target", json!("x86_64-unknown-linux-gnu")),
        ("profile", json!("debug")),
        ("config_sha256", json!("a".repeat(64))),
    ] {
        let mut changed = identity.clone();
        changed[key] = value;
        assert!(
            validate_identity(&receipt.unit, &parse_identity(&changed).unwrap()).is_err(),
            "{key}"
        );
    }
    let mut oversized = identity;
    oversized["rustc"] = json!("x".repeat(4097));
    assert!(parse_identity(&oversized).is_err());
}

#[test]
fn complete_catalog_refuses_crossed_lane_source_member_and_unknown_keys() {
    let (catalog, _) = fixture(UiComponent::Interactions);
    assert!(inspect("interactions", "stable", &catalog).is_err());
    assert!(inspect("command-kernel", "canary", &catalog).is_err());
    for (path, value) in [
        ("/runtime", json!("native")), ("/product_id", json!("rw.datasolutions.module.interactions")),
        ("/assets/0/platform", json!("linux")), ("/assets/0/sha256", json!("A".repeat(64))),
        ("/assets/0/source_revisions", json!({"ds-command-kernel":"b".repeat(40)})),
        ("/assets/1/source_sha", json!("c".repeat(40))),
        ("/assets/1/built_at", json!("2026-10-14T08:00:00Z")),
        ("/assets/1/engines/0/manifest_sha256", json!("f".repeat(64))),
        ("/assets/0/engines/0/release", json!("ds-interactions@0.2.0+bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb")),
        ("/assets/0/url", json!("https://storage.googleapis.com/another-bucket/modules/interactions/canary/wasm/0.1.0-r1/ds_interactions_wasm.js")),
        ("/assets/0/url", json!("https://storage.googleapis.com/dedicated-public-releases/modules/interactions/stable/wasm/0.1.0-r1/ds_interactions_wasm.js")),
    ] {
        let mut changed = catalog.clone(); *changed.pointer_mut(path).unwrap() = value;
        assert!(inspect("interactions", "canary", &changed).is_err(), "{path}");
    }
    let mut missing = catalog.clone();
    missing["assets"].as_array_mut().unwrap().pop();
    assert!(inspect("interactions", "canary", &missing).is_err());
    let mut duplicate = catalog.clone();
    duplicate["assets"][3] = duplicate["assets"][0].clone();
    assert!(inspect("interactions", "canary", &duplicate).is_err());
    // Owner 22 Oct: members a newer publisher adds are accepted and never read
    // (these used to be refusals); the admitted unit is unchanged. The
    // producer-bound build_manifest stays closed (see the next test).
    let admitted = inspect("interactions", "canary", &catalog).unwrap();
    for path in ["", "/assets/0", "/assets/0/engines/0"] {
        let mut additive = catalog.clone();
        additive.pointer_mut(path).unwrap()["future"] = json!(true);
        assert_eq!(
            inspect("interactions", "canary", &additive).unwrap(),
            admitted,
            "{path}"
        );
    }
    let mut oversized = catalog.clone();
    oversized["future"] = json!("x".repeat(8001));
    assert!(inspect("interactions", "canary", &oversized).is_err());
    let mut missing = catalog;
    missing["assets"][0].as_object_mut().unwrap().remove("url");
    assert!(inspect("interactions", "canary", &missing).is_err());
}

#[test]
fn trusted_catalog_manifest_is_required_before_transfer_or_adapter_evaluation() {
    let (catalog, identity) = fixture(UiComponent::Interactions);
    let mut missing = catalog.clone();
    missing.as_object_mut().unwrap().remove("build_manifest");
    assert!(inspect("interactions", "canary", &missing).is_err());
    let mut unknown = catalog.clone();
    unknown["build_manifest"]["future"] = json!(true);
    assert!(inspect("interactions", "canary", &unknown).is_err());
    for (key, value) in [
        ("schema", json!("ds.engine-build/v1")),
        ("component", json!("command-kernel")),
        ("source_sha", json!("a".repeat(40))),
        ("ui_contract", json!(2)),
        ("target", json!("x86_64-unknown-linux-gnu")),
        ("profile", json!("debug")),
        ("cargo_lock_sha256", json!("a".repeat(64))),
        ("config_sha256", json!("a".repeat(64))),
        ("rustc", json!("rustc 1.91.0 (1159e78c4 2025-09-14)")),
    ] {
        let mut tampered = catalog.clone();
        tampered["build_manifest"][key] = value;
        assert!(
            inspect("interactions", "canary", &tampered).is_err(),
            "{key}"
        );
    }
    let unit = inspect("interactions", "canary", &catalog).unwrap();
    let (prepared, _, _) = ready();
    assert_eq!(prepared.engine_identity, unit.published().unwrap().build_manifest);
    assert_eq!(
        serde_json::to_value(&prepared.engine_identity).unwrap(),
        identity
    );
}

#[test]
fn bytes_precede_loaded_identity_and_bind_all_four_members() {
    let (receipt, verified, identity) = ready();
    let selection = activate(None, receipt.clone(), 0, true, guard()).unwrap();
    let captured = capture(
        Some(&selection),
        UiComponent::Interactions,
        "canary",
        "https://gateway.example",
    )
    .unwrap();
    admit_loaded(&captured, &verified, &identity).unwrap();
    let mut corrupted = verified.clone();
    corrupted[0].sha256 = "a".repeat(64);
    assert_eq!(
        admit_loaded(&captured, &corrupted, &Value::Null)
            .unwrap_err()
            .code,
        "module_artifact_invalid"
    );
    assert!(verify_files(&receipt.unit, &verified[..3]).is_err());
    let mut duplicate = verified.clone();
    duplicate[3] = duplicate[0].clone();
    assert!(verify_files(&receipt.unit, &duplicate).is_err());
    let mut tampered = receipt.unit.clone();
    if let UiUnit::Published { total_bytes, .. } = &mut tampered { *total_bytes += 1; }
    assert!(verify_files(&tampered, &verified).is_err());
    assert!(admit_loaded(&captured, &verified, &json!({})).is_err());
}

#[test]
fn held_generation_lane_authority_and_guard_fence_offline_transitions() {
    let (receipt, _, _) = ready();
    assert!(prepare(
        receipt.unit.clone(),
        receipt.authority.clone(),
        &files(&receipt.unit),
        false
    )
    .is_err());
    let first = activate(None, receipt.clone(), 0, true, guard()).unwrap();
    assert!(activate(Some(&first), receipt.clone(), 0, true, guard()).is_err());
    let mut dirty = guard();
    dirty.unsaved_edits = true;
    assert_eq!(
        activate(Some(&first), receipt.clone(), 1, true, dirty)
            .unwrap_err()
            .code,
        "module_activation_deferred"
    );
    assert!(admit_retained(&receipt, "stable", "https://gateway.example").is_err());
    assert!(admit_retained(&receipt, "canary", "https://other.example").is_err());
    assert!(capture(
        Some(&first),
        UiComponent::CommandKernel,
        "canary",
        "https://gateway.example"
    )
    .is_err());
    // A second complete immutable version retains the former active unit.
    let mut next_catalog = serde_json::to_value(&receipt.unit.published().unwrap()).unwrap();
    next_catalog["version"] = json!("0.1.0-r2");
    for asset in next_catalog["assets"].as_array_mut().unwrap() {
        let url = asset["url"]
            .as_str()
            .unwrap()
            .replace("/0.1.0-r1/", "/0.1.0-r2/");
        asset["url"] = json!(url);
    }
    let next_unit = inspect("interactions", "canary", &next_catalog).unwrap();
    let next = prepare(
        next_unit.clone(),
        receipt.authority.clone(),
        &files(&next_unit),
        true,
    )
    .unwrap();
    let advanced = activate(Some(&first), next, 1, true, guard()).unwrap();
    let rolled_back = rollback(&advanced, 2, true, guard()).unwrap();
    assert_eq!(
        capture(
            Some(&rolled_back),
            UiComponent::Interactions,
            "canary",
            "https://gateway.example"
        )
        .unwrap()
        .receipt,
        receipt
    );
    assert_eq!(rolled_back.generation, 3);
    let removed = remove(&first, 1, true).unwrap();
    assert_eq!(
        capture(
            Some(&removed),
            UiComponent::Interactions,
            "canary",
            "https://gateway.example"
        )
        .unwrap_err()
        .code,
        "module_removed"
    );
    let restored = rollback(&removed, 2, true, guard()).unwrap();
    assert_eq!(restored.generation, 3);
    assert_eq!(
        capture(
            Some(&restored),
            UiComponent::Interactions,
            "canary",
            "https://gateway.example"
        )
        .unwrap()
        .receipt,
        receipt
    );
    let mut corrupt = restored.clone();
    if let UiSelectionState::Installed { active, .. } = &mut corrupt.selection {
        active.engine_identity.config_sha256 = "f".repeat(64);
    }
    assert!(validate_selection(&corrupt, UiComponent::Interactions, "canary").is_err());
}

#[test]
fn acquisition_plan_is_bounded_and_withdrawn_catalogs_stay_visible() {
    let (receipt, _, _) = ready();
    let unit = &receipt.unit;
    assert_eq!(
        acquisition_plan(unit, "canary", unit.unit_sha256(), true)
            .unwrap()
            .len(),
        4
    );
    assert!(acquisition_plan(unit, "canary", unit.unit_sha256(), false).is_err());
    assert!(acquisition_plan(unit, "stable", unit.unit_sha256(), true).is_err());
    assert!(acquisition_plan(unit, "canary", &"f".repeat(64), true).is_err());
    for status in ["blocked", "stopped", "rolled_back"] {
        let mut release = serde_json::to_value(&unit.published().unwrap()).unwrap();
        release["status"] = json!(status);
        let withdrawn = inspect("interactions", "canary", &release).unwrap();
        assert_eq!(withdrawn.published().unwrap().status, status);
        assert!(acquisition_plan(&withdrawn, "canary", withdrawn.unit_sha256(), true).is_err());
    }
}

/// Moved from module_engine_tests.rs, which ds-module-admission also compiles
/// and which therefore cannot name Kernel-only owners such as `table_edit`.
#[test]
fn different_contract_metadata_admits_the_same_working_typed_exchange() {
    use crate::module_engine::{admit_ui_identity, parse_ui_identity};
    use crate::table_edit::{
        decode_request, decode_response, AddColumnEditResult, TableEditRequest, TableEditResponse,
    };
    // Owner ruling and module-releases.md: metadata equality is not the
    // compatibility criterion. Preserve all producer-integrity checks.
    for version in [1, 2] {
        let mut value: serde_json::Value =
            serde_json::from_str(include_str!("fixtures/interactions-ui-engine-build.json"))
                .unwrap();
        value["component"] = serde_json::to_value(UiComponent::CommandKernel).unwrap();
        value["ui_contract"] = json!(version);
        let identity = parse_ui_identity(&value).unwrap();
        admit_ui_identity(UiComponent::CommandKernel, &identity, None).unwrap();
        let request: TableEditRequest = serde_json::from_value(json!({
            "operation":"add_column",
            "input":{"rows":[{}],"column":"n","data_type":"int","default_value":"3.5"}
        }))
        .unwrap();
        let request = decode_request(&serde_json::to_vec(&request).unwrap()).unwrap();
        // The Network table owner's plan (ds-geo, a dev-dependency here).
        let TableEditRequest::AddColumn(input) = &request else {
            unreachable!("an add_column request")
        };
        let reply = TableEditResponse::AddColumn(AddColumnEditResult {
            plan: ds_geo::table::add_column(input.clone()).unwrap(),
            default_value: ds_geo::table::coerce_value(&input.default_value, input.data_type)
                .unwrap(),
        });
        let decoded = decode_response(&request, &serde_json::to_vec(&reply).unwrap()).unwrap();
        assert_eq!(
            serde_json::to_value(decoded).unwrap()["result"]["default_value"],
            4
        );
    }
}
