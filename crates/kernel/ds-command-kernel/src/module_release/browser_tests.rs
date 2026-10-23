use super::*;
use serde_json::json;
fn catalog() -> Value {
    serde_json::from_str::<Value>(include_str!("fixtures/network-wasm-canary.json")).unwrap()
        ["release"]
        .clone()
}
fn ready() -> InstalledBrowserModule {
    let unit = inspect("canary", &catalog()).unwrap();
    let verified = unit
        .release
        .assets
        .iter()
        .map(|asset| VerifiedFile {
            filename: filename(asset).unwrap(),
            sha256: asset.sha256.clone(),
            size_bytes: asset.size_bytes,
        })
        .collect::<Vec<_>>();
    let identity = json!({"schema":"ds.network-engine/v1","protocolMajor":1,"protocolMinor":0,"environment":"wasm","runtime":{"environment":"wasm","logicalCpus":1,"internalWorkerThreads":1},"coreVersion":"0.1.0","sourceSha":unit.release.assets[0].source_sha,"dependencyClosureSha256":unit.release.assets[0].engines[0].manifest_sha256,"jsonObjectOrder":"sorted","capabilities":["lv_processing_v1"]});
    prepare(
        unit,
        BrowserAuthority {
            gateway_origin: "https://gateway.example".into(),
            route_manifest_sha256: "a".repeat(64),
        },
        &verified,
        identity,
        true,
    )
    .unwrap()
}
#[test]
fn captures_only_an_active_admitted_unit_and_keeps_it_across_removal() {
    let receipt = ready();
    let guard = ActivationGuard {
        unsaved_edits: false,
        uncaptured_sessions: 0,
        host_restart_required: false,
    };
    assert_eq!(
        capture(None, "canary", &receipt.authority.gateway_origin)
            .unwrap_err()
            .code,
        "module_not_installed"
    );
    let selected = activate(None, receipt.clone(), 0, true, guard).unwrap();
    let captured = capture(Some(&selected), "canary", &receipt.authority.gateway_origin).unwrap();
    assert_eq!(captured.generation, 1);
    assert_eq!(captured.receipt, receipt);
    assert!(capture(Some(&selected), "stable", &receipt.authority.gateway_origin).is_err());
    assert!(capture(Some(&selected), "canary", "https://other.example").is_err());
    let removed = remove(&selected, 1, true).unwrap();
    assert_eq!(
        capture(Some(&removed), "canary", &receipt.authority.gateway_origin)
            .unwrap_err()
            .code,
        "module_removed"
    );
    let verified = receipt
        .unit
        .release
        .assets
        .iter()
        .map(|asset| VerifiedFile {
            filename: filename(asset).unwrap(),
            sha256: asset.sha256.clone(),
            size_bytes: asset.size_bytes,
        })
        .collect::<Vec<_>>();
    // Already captured jobs keep their unit. New jobs see the removed state.
    admit_loaded(&captured, &verified, &receipt.engine_identity).unwrap();
    let mut corrupt = verified.clone();
    corrupt[0].sha256 = "0".repeat(64);
    assert_eq!(
        admit_loaded(&captured, &corrupt, &receipt.engine_identity)
            .unwrap_err()
            .code,
        "module_artifact_invalid"
    );
    let mut wrong_identity = receipt.engine_identity.clone();
    wrong_identity["sourceSha"] = json!("0".repeat(40));
    assert_eq!(
        admit_loaded(&captured, &verified, &wrong_identity)
            .unwrap_err()
            .code,
        "module_engine_incompatible"
    );
    let mut bad_generation = captured;
    bad_generation.generation = 0;
    assert_eq!(
        admit_loaded(&bad_generation, &verified, &receipt.engine_identity)
            .unwrap_err()
            .code,
        "module_receipt_invalid"
    );
}
#[test]
fn published_complete_unit_has_stable_identity_without_reinterpreting_native_catalogs() {
    let value = catalog();
    let unit = inspect("canary", &value).unwrap();
    assert_eq!(unit.total_bytes, 26_795_337);
    let mut reversed = value.clone();
    reversed["assets"].as_array_mut().unwrap().reverse();
    assert_eq!(
        unit.unit_sha256,
        inspect("canary", &reversed).unwrap().unit_sha256
    );
    assert!(validate_module_release("network", "canary", &value).is_err());
    assert!(inspect("stable", &value).is_err());
    let mut native = value.clone();
    native["runtime"] = json!("native");
    assert!(inspect("canary", &native).is_err());
}
#[test]
fn acquisition_requires_exact_consent_lane_and_current_complete_catalog() {
    let unit = inspect("canary", &catalog()).unwrap();
    assert_eq!(
        acquisition_plan(&unit, "canary", &unit.unit_sha256, true)
            .unwrap()
            .len(),
        4
    );
    assert_eq!(
        acquisition_plan(&unit, "canary", &unit.unit_sha256, false)
            .unwrap_err()
            .code,
        "module_consent_required"
    );
    assert!(acquisition_plan(&unit, "stable", &unit.unit_sha256, true).is_err());
    assert_eq!(
        acquisition_plan(&unit, "canary", &"a".repeat(64), true)
            .unwrap_err()
            .code,
        "module_selection_changed"
    );
    for component in ["solar"] {
        assert!(admit_target(component, "canary").is_err());
    }
    let mut withdrawn = catalog();
    withdrawn["status"] = json!("blocked");
    let unit = inspect("canary", &withdrawn).unwrap();
    assert_eq!(
        acquisition_plan(&unit, "canary", &unit.unit_sha256, true)
            .unwrap_err()
            .code,
        "module_release_withdrawn"
    );
}
#[test]
fn incomplete_crossed_mixed_oversized_and_unknown_members_refuse() {
    let base = catalog();
    let mut cases = Vec::new();
    let mut v = base.clone();
    v["assets"].as_array_mut().unwrap().pop();
    cases.push(v);
    let mut v = base.clone();
    v["assets"][1] = v["assets"][0].clone();
    cases.push(v);
    let mut v = base.clone();
    v["assets"][0]["source_revisions"]["ds-command-kernel"] = json!("a".repeat(40));
    cases.push(v);
    let mut v = base.clone();
    v["assets"][0]["url"] = json!("https://storage.googleapis.com/bucket/modules/network/stable/wasm/0.1.0-r1/ds_network_wasm.d.ts");
    cases.push(v);
    let mut v = base.clone();
    v["assets"][0]["size_bytes"] = json!(MAX_UNIT_BYTES);
    cases.push(v);
    let mut v = base.clone();
    v["assets"][0]["built_at"] = json!("bad");
    cases.push(v);
    let mut v = base.clone();
    v["assets"][0]["url"] = json!(format!(
        "{}?redirect=bad",
        v["assets"][0]["url"].as_str().unwrap()
    ));
    cases.push(v);
    // Owner 22 Oct: members a newer publisher adds are accepted and never read
    // (these two cases used to be refusals); the admitted unit is unchanged.
    let admitted = inspect("canary", &base).unwrap();
    let mut v = base.clone();
    v["assets"][0]["executable"] = json!("evil");
    v["extra"] = json!(true);
    v["assets"][1]["engines"][0]["future"] = json!({"signature": "x"});
    assert_eq!(inspect("canary", &v).unwrap(), admitted);
    // Added members stay bounded; a missing required member still refuses.
    let mut v = base.clone();
    v["extra"] = json!("x".repeat(8001));
    cases.push(v);
    let mut v = base.clone();
    for index in 0..33 {
        v[format!("future_{index}")] = json!(index);
    }
    cases.push(v);
    let mut v = base;
    v["assets"][0].as_object_mut().unwrap().remove("sha256");
    cases.push(v);
    for value in cases {
        assert!(inspect("canary", &value).is_err(), "{value}");
    }
}
#[test]
fn staging_requires_consent_all_verified_bytes_and_exact_loaded_identity() {
    let receipt = ready();
    let files = receipt
        .unit
        .release
        .assets
        .iter()
        .map(|asset| VerifiedFile {
            filename: filename(asset).unwrap(),
            sha256: asset.sha256.clone(),
            size_bytes: asset.size_bytes,
        })
        .collect::<Vec<_>>();
    let stage = |unit, verified: &[VerifiedFile], identity, consent| {
        prepare(unit, receipt.authority.clone(), verified, identity, consent)
    };
    assert_eq!(
        stage(
            receipt.unit.clone(),
            &files,
            receipt.engine_identity.clone(),
            false
        )
        .unwrap_err()
        .code,
        "module_consent_required"
    );
    assert!(stage(
        receipt.unit.clone(),
        &files[..3],
        receipt.engine_identity.clone(),
        true
    )
    .is_err());
    let mut corrupt = files.clone();
    corrupt[0].sha256 = "b".repeat(64);
    assert!(stage(
        receipt.unit.clone(),
        &corrupt,
        receipt.engine_identity.clone(),
        true
    )
    .is_err());
    for (key, value) in [
        ("environment", json!("native")),
        ("sourceSha", json!("c".repeat(40))),
        ("dependencyClosureSha256", json!("c".repeat(64))),
        ("coreVersion", json!("0.2.0")),
        ("jsonObjectOrder", json!("unknown")),
        ("capabilities", json!([])),
    ] {
        let mut identity = receipt.engine_identity.clone();
        identity[key] = value;
        assert!(stage(receipt.unit.clone(), &files, identity, true).is_err());
    }
    let mut unit = receipt.unit.clone();
    unit.unit_sha256 = "d".repeat(64);
    assert!(stage(unit, &files, receipt.engine_identity.clone(), true).is_err());
    let mut unit = receipt.unit.clone();
    unit.release.status = "blocked".into();
    assert_eq!(
        stage(unit, &files, receipt.engine_identity.clone(), true)
            .unwrap_err()
            .code,
        "module_release_withdrawn"
    );
}
#[test]
fn retained_receipt_requires_this_hosts_exact_lane_and_authority() {
    let receipt = ready();
    assert_eq!(
        admit_retained(&receipt, "canary", "https://gateway.example").unwrap(),
        receipt
    );
    assert!(admit_retained(&receipt, "stable", "https://gateway.example").is_err());
    assert!(admit_retained(&receipt, "canary", "https://other.example").is_err());
    let wire = serde_json::to_vec(&json!({"kind":"installation","receipt":receipt,"channel":"canary","gateway_origin":"https://gateway.example"})).unwrap();
    let result: InstalledBrowserModule = serde_json::from_str(&evaluate(&wire).unwrap()).unwrap();
    assert_eq!(result, receipt);
}
#[test]
fn byte_verification_precedes_loaded_identity_and_rejects_tampered_units() {
    let receipt = ready();
    let mut files = receipt
        .unit
        .release
        .assets
        .iter()
        .map(|asset| VerifiedFile {
            filename: filename(asset).unwrap(),
            sha256: asset.sha256.clone(),
            size_bytes: asset.size_bytes,
        })
        .collect::<Vec<_>>();
    let wire = |unit: &BrowserUnit, files: &[VerifiedFile]| {
        serde_json::to_vec(&json!({"kind":"verify_files","unit":unit,"verified":files})).unwrap()
    };
    assert_eq!(evaluate(&wire(&receipt.unit, &files)).unwrap(), "null");
    files[0].size_bytes += 1;
    assert!(evaluate(&wire(&receipt.unit, &files))
        .unwrap_err()
        .starts_with("module_artifact_invalid"));
    files[0].size_bytes -= 1;
    files[0].filename = files[1].filename.clone();
    assert!(verify_files(&receipt.unit, &files).is_err());
    let mut unit = receipt.unit.clone();
    unit.total_bytes += 1;
    assert!(verify_files(&unit, &[]).is_err());
    let mut withdrawn = receipt;
    withdrawn.unit.release.status = "blocked".into();
    withdrawn.unit = inspect(
        "canary",
        &serde_json::to_value(&withdrawn.unit.release).unwrap(),
    )
    .unwrap();
    assert!(activate(None, withdrawn, 0, true, ActivationGuard::default()).is_err());
}
#[test]
fn selection_fences_held_work_and_supports_explicit_offline_remove_restore() {
    let receipt = ready();
    let selected = activate(None, receipt.clone(), 0, true, ActivationGuard::default()).unwrap();
    assert_eq!(selected.generation, 1);
    assert_eq!(
        activate(
            Some(&selected),
            receipt.clone(),
            1,
            true,
            ActivationGuard::default()
        )
        .unwrap(),
        selected
    );
    assert!(activate(
        Some(&selected),
        receipt.clone(),
        0,
        true,
        ActivationGuard::default()
    )
    .is_err());
    assert!(activate(
        None,
        receipt,
        0,
        true,
        ActivationGuard {
            unsaved_edits: true,
            ..Default::default()
        }
    )
    .is_err());
    assert!(remove(&selected, 1, false).is_err());
    let removed = remove(&selected, 1, true).unwrap();
    assert_eq!(removed.generation, 2);
    assert!(matches!(
        removed.selection,
        BrowserSelectionState::Removed { .. }
    ));
    assert!(remove(&removed, 1, true).is_err());
    assert!(rollback(&removed, 2, false, ActivationGuard::default()).is_err());
    let restored = rollback(&removed, 2, true, ActivationGuard::default()).unwrap();
    assert_eq!(restored.generation, 3);
    assert!(matches!(
        restored.selection,
        BrowserSelectionState::Installed { .. }
    ));
    assert!(validate_selection(&restored, "stable").is_err());
    let mut corrupt = restored;
    corrupt.generation = MAX_GENERATION;
    assert!(remove(&corrupt, MAX_GENERATION, true).is_err());
}
#[test]
fn wire_is_closed_bounded_and_exposes_the_same_inspection_owner() {
    let wire =
        serde_json::to_vec(&json!({"kind":"inspect","channel":"canary","release":catalog()}))
            .unwrap();
    let unit: BrowserUnit = serde_json::from_str(&evaluate(&wire).unwrap()).unwrap();
    assert_eq!(unit, inspect("canary", &catalog()).unwrap());
    for input in [
        b"{}".as_slice(),
        b"{\"kind\":\"selection\",\"channel\":\"canary\",\"current\":null,\"unknown\":true}",
    ] {
        assert!(evaluate(input).is_err());
    }
    assert!(evaluate(&vec![b' '; MAX_WIRE_BYTES + 1]).is_err());
}
fn reporter_catalog() -> Value {
    serde_json::from_str::<Value>(include_str!("fixtures/reporter-wasm-canary.json")).unwrap()
        ["release"]
        .clone()
}
fn reporter_ready() -> InstalledBrowserModule {
    let unit = inspect("canary", &reporter_catalog()).unwrap();
    let verified = unit
        .release
        .assets
        .iter()
        .map(|asset| VerifiedFile {
            filename: filename(asset).unwrap(),
            sha256: asset.sha256.clone(),
            size_bytes: asset.size_bytes,
        })
        .collect::<Vec<_>>();
    let identity = json!({"schema":"ds.reporter-engine/v1","protocolMajor":1,"environment":"wasm",
        "coreVersion":"0.1.0","sourceSha":unit.release.assets[0].source_sha,
        "dependencyClosureSha256":unit.release.assets[0].engines[0].manifest_sha256,
        "capabilities":["render_held_svg_pdf_v1"]});
    prepare(
        unit,
        BrowserAuthority {
            gateway_origin: "https://gateway.example".into(),
            route_manifest_sha256: "a".repeat(64),
        },
        &verified,
        identity,
        true,
    )
    .unwrap()
}
#[test]
fn reporter_requires_its_complete_closed_unit_owner_protocol_and_identity() {
    let receipt = reporter_ready();
    admit_target("reporter", "canary").unwrap();
    assert!(admit_target("solar", "canary").is_err());
    let files = receipt
        .unit
        .release
        .assets
        .iter()
        .map(|asset| VerifiedFile {
            filename: filename(asset).unwrap(),
            sha256: asset.sha256.clone(),
            size_bytes: asset.size_bytes,
        })
        .collect::<Vec<_>>();
    for (key, value) in [
        ("schema", json!("ds.network-engine/v1")),
        ("protocolMajor", json!("1")),
        ("environment", json!("native")),
        ("coreVersion", json!("0.2.0")),
        ("sourceSha", json!("a".repeat(40))),
        ("dependencyClosureSha256", json!("a".repeat(64))),
        ("capabilities", json!(["lv_processing_v1"])),
        // Owner 22 Oct (B5): an added capability is admitted (below); a
        // repeated token is not a declaration.
        (
            "capabilities",
            json!(["render_held_svg_pdf_v1", "render_held_svg_pdf_v1"]),
        ),
    ] {
        let mut identity = receipt.engine_identity.clone();
        identity[key] = value;
        assert!(
            prepare(
                receipt.unit.clone(),
                receipt.authority.clone(),
                &files,
                identity,
                true
            )
            .is_err(),
            "{key}"
        );
    }
    for key in [
        "schema",
        "protocolMajor",
        "environment",
        "coreVersion",
        "sourceSha",
        "dependencyClosureSha256",
        "capabilities",
    ] {
        let mut identity = receipt.engine_identity.clone();
        identity.as_object_mut().unwrap().remove(key);
        assert!(
            prepare(
                receipt.unit.clone(),
                receipt.authority.clone(),
                &files,
                identity,
                true
            )
            .is_err(),
            "{key}"
        );
    }
    // Owner 22 Oct (B5): a Reporter unit that declares the consumed capability
    // is admitted with added capabilities, another protocol integer and an
    // added identity fact (the first used to be a refusal above).
    let mut newer = receipt.engine_identity.clone();
    newer["capabilities"] = json!(["render_held_svg_pdf_v1", "render_held_png_v1"]);
    newer["protocolMajor"] = json!(2);
    newer["futureFact"] = json!(true);
    let admitted = prepare(
        receipt.unit.clone(),
        receipt.authority.clone(),
        &files,
        newer.clone(),
        true,
    )
    .unwrap();
    assert_eq!(admitted.unit, receipt.unit);
    assert_eq!(admitted.engine_identity, newer);
    for index in 0..4 {
        let mut corrupt = files.clone();
        corrupt[index].sha256 = "0".repeat(64);
        assert_eq!(
            verify_files(&receipt.unit, &corrupt).unwrap_err().code,
            "module_artifact_invalid"
        );
    }
    assert!(verify_files(&receipt.unit, &files[..3]).is_err());
    let mut wrong = reporter_catalog();
    wrong["assets"][0]["kind"] = json!("module-executable");
    assert!(inspect("canary", &wrong).is_err());
    let mut wrong = reporter_catalog();
    wrong["protocol"] = json!(PROTOCOL);
    assert!(inspect("canary", &wrong).is_err());
    let mut wrong = reporter_catalog();
    wrong["assets"][0]["engines"][0]["engine"] = json!("ds-network");
    assert!(inspect("canary", &wrong).is_err());
    let selected = activate(None, receipt.clone(), 0, true, ActivationGuard::default()).unwrap();
    let captured = capture(Some(&selected), "canary", "https://gateway.example").unwrap();
    admit_loaded(&captured, &files, &receipt.engine_identity).unwrap();
}
#[test]
fn browser_components_have_independent_updates_and_refuse_crossed_selections() {
    let network = ready();
    let reporter = reporter_ready();
    let n = activate(None, network.clone(), 0, true, ActivationGuard::default()).unwrap();
    let r = activate(None, reporter.clone(), 0, true, ActivationGuard::default()).unwrap();
    assert!(activate(
        Some(&n),
        reporter.clone(),
        1,
        true,
        ActivationGuard::default()
    )
    .is_err());
    assert!(activate(
        Some(&r),
        network.clone(),
        1,
        true,
        ActivationGuard::default()
    )
    .is_err());
    for previous in [false, true] {
        let mut crossed = n.clone();
        if let BrowserSelectionState::Installed {
            active,
            previous: old,
        } = &mut crossed.selection
        {
            if previous {
                *old = Some(Box::new(reporter.clone()));
            } else {
                *active = Box::new(reporter.clone());
            }
        }
        assert!(validate_selection(&crossed, "canary").is_err());
    }
    let mut newer = reporter.clone();
    let mut release = reporter_catalog();
    release["version"] = json!("0.1.0-r99");
    for asset in release["assets"].as_array_mut().unwrap() {
        asset["url"] = json!(asset["url"]
            .as_str()
            .unwrap()
            .replace("/0.1.0-r1/", "/0.1.0-r99/"));
        asset["source_revisions"]["ds-command-kernel"] = json!("9".repeat(40));
    }
    newer.unit = inspect("canary", &release).unwrap();
    let updated = activate(Some(&r), newer, 1, true, ActivationGuard::default()).unwrap();
    let rolled = rollback(&updated, 2, true, ActivationGuard::default()).unwrap();
    assert_eq!(n.generation, 1);
    assert_eq!(rolled.generation, 3);
    assert!(
        matches!(rolled.selection,BrowserSelectionState::Installed{active,..} if *active==reporter)
    );
}
#[test]
fn wire_context_defaults_network_and_rejects_crossed_component_callers() {
    let receipt = reporter_ready();
    let selected = activate(None, receipt.clone(), 0, true, ActivationGuard::default()).unwrap();
    for kind in ["inspect", "selection", "capture", "installation"] {
        let mut request = match kind {
            "inspect" => json!({"kind":kind,"channel":"canary","release":reporter_catalog()}),
            "selection" => json!({"kind":kind,"channel":"canary","current":selected}),
            "capture" => {
                json!({"kind":kind,"channel":"canary","current":selected,"gateway_origin":"https://gateway.example"})
            }
            _ => {
                json!({"kind":kind,"channel":"canary","receipt":receipt,"gateway_origin":"https://gateway.example"})
            }
        };
        assert!(
            evaluate(&serde_json::to_vec(&request).unwrap()).is_err(),
            "{kind}"
        );
        request["component"] = json!("reporter");
        assert!(
            evaluate(&serde_json::to_vec(&request).unwrap()).is_ok(),
            "{kind}"
        );
        request["component"] = json!("solar");
        assert!(
            evaluate(&serde_json::to_vec(&request).unwrap()).is_err(),
            "{kind}"
        );
    }
}
