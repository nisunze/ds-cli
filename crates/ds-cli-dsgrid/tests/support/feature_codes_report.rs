use super::*;
use ds_cli_contract::{Format, Output};
use ds_grid_model::*;
use std::{fs, path::Path};

fn context() -> Context {
    Context {
        confirmed: false,
        output: Output::resolve(Format::Json, false, true),
    }
}

fn inputs(args: &[&str]) -> Inputs {
    ds_cli_contract::parse(
        &COMMAND,
        &args.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
    )
    .unwrap()
}

fn fixture(rich: bool) -> Vec<u8> {
    let blank = ds_grid_exchange::create_blank_model(&ds_grid_exchange::BlankModelRequest {
        model_id: Some("feature-code-process-fixture"),
        coordinate_system: Some("EPSG:32735"),
        ..Default::default()
    })
    .unwrap();
    if !rich {
        return blank.bytes;
    }
    let mut package = ds_grid_exchange::package::unpack(&blank.bytes).unwrap();
    for (token, number) in [
        ("UNKNOWN", Some(999)),
        ("FOREST", Some(51)),
        ("GROUND", Some(200)),
        ("LEGACY", None),
    ] {
        package.snapshot.feature_codes.push(FeatureCodeRow {
            id: FeatureCodeId::new(token).unwrap(),
            code_token: token.into(),
            name: format!("{token} fixture"),
            description: None,
            namespace: Some("fixture-standard".into()),
            applies_to_points: true,
            applies_to_lines: true,
            applies_to_polygons: false,
            survey_form: None,
            plan_policy: PlanPresentationPolicy {
                visible: true,
                color: None,
                symbol: None,
                point_radius_px: None,
                line_width_px: None,
            },
            profile_policy: ProfilePresentationPolicy {
                visible: true,
                color: None,
                marker: None,
                show_ordinate: false,
            },
            label_policy: SelectiveLabelPolicy {
                show_by_default: false,
                source: LabelSource::CodeToken,
                min_zoom: None,
            },
            superseded_by: (token == "LEGACY").then(|| FeatureCodeId::new("GROUND").unwrap()),
            clearance: number.map(|code_number| FeatureCodeClearance {
                code_number,
                required_vertical_m: 6.25,
                required_horizontal_m: 3.5,
                tin_member: token == "GROUND",
                obstacle_height_default_m: Some(2.0),
                cleared_ground_additional_clearance_m: (token == "FOREST").then_some(1.75),
                assumed: token == "FOREST",
                voltage_class: Some("MV".into()),
                source: Some("synthetic parity fixture".into()),
                fea_native: None,
            }),
        });
    }
    for (i, token) in [
        "FOREST",
        "GROUND",
        "UNKNOWN",
        "LEGACY",
        "unresolved-z",
        "unresolved-a",
        "unresolved-z",
    ]
    .into_iter()
    .enumerate()
    {
        package.snapshot.terrain_points.push(TerrainPointRow {
            id: TerrainPointId::new(format!("point-{i}")).unwrap(),
            x_m: 500_000.0 + i as f64,
            y_m: 9_800_000.0,
            z_m: 100.0,
            feature_class: token.into(),
            description: None,
            required_clearance_m: None,
            source_id: None,
        });
    }
    let vertex = Vertex3 {
        x_m: 500_000.0,
        y_m: 9_800_000.0,
        z_m: 100.0,
    };
    package
        .snapshot
        .terrain_breaklines
        .push(TerrainBreaklineRow {
            id: BreaklineId::new("breakline").unwrap(),
            feature_class: "GROUND".into(),
            vertices: vec![
                vertex,
                Vertex3 {
                    x_m: 500_001.0,
                    ..vertex
                },
            ],
        });
    package.snapshot.crossings.push(CrossingRow {
        id: CrossingId::new("crossing").unwrap(),
        feature_class: "unresolved-a".into(),
        description: None,
        vertices: vec![vertex],
        rings: vec![],
        required_clearance_m: None,
    });
    let model = package.manifest.model;
    ds_grid_exchange::package::pack(
        &package.snapshot,
        &ds_grid_exchange::PackOptions {
            model_id: model.model_id,
            model_revision: 7,
            presentation: model.presentation,
            coordinate_system: model.coordinate_system,
            library_pins: model.library_pins,
            library_needs: model.library_needs,
            assets: package.assets,
            exchange_bindings: package.exchange_bindings,
        },
    )
    .unwrap()
}

fn old_report(bytes: &[u8]) -> Value {
    let package = ds_grid_exchange::package::unpack(bytes).unwrap();
    let session = ds_grid_exchange::linked_models::open_session(&package);
    serde_json::to_value(ds_grid_engine::feature_code_report(session.snapshot())).unwrap()
}

#[test]
fn projection_requires_complete_typed_counts_and_lists_and_preserves_owner_rows() {
    let mut report = old_report(&fixture(true));
    report["codes"][0]["future_owner_fact"] = json!({"kept": [3, 2, 1]});
    let answer = project(json!({}), "captured-version".into(), report.clone(), 1).unwrap();
    assert_eq!(answer["engine"], "captured-version");
    assert_eq!(answer["codes"][0], report["codes"][0]);
    assert_eq!(
        answer["unresolved_tokens"][0],
        report["unresolved_tokens"][0]
    );
    assert_eq!(
        answer["more"]["truncated"],
        json!([
            {"field":"codes", "total":4, "shown":1, "withheld":3, "limit":1},
            {"field":"unresolved_tokens", "total":2, "shown":1, "withheld":1, "limit":1}
        ])
    );
    for key in report.as_object().unwrap().keys() {
        let mut missing = report.clone();
        missing.as_object_mut().unwrap().remove(key);
        assert_eq!(
            project(json!({}), "v".into(), missing, 1)
                .unwrap_err()
                .code(),
            "network_result_invalid",
            "{key}"
        );
        let mut mistyped = report.clone();
        mistyped[key] = Value::Null;
        assert_eq!(
            project(json!({}), "v".into(), mistyped, 1)
                .unwrap_err()
                .code(),
            "network_result_invalid",
            "{key}"
        );
    }
    for key in ["code_count", "unresolved_token_count"] {
        let mut incomplete = report.clone();
        incomplete[key] = json!(999);
        assert_eq!(
            project(json!({}), "v".into(), incomplete, 1)
                .unwrap_err()
                .code(),
            "network_result_invalid"
        );
    }
    for key in ["codes", "unresolved_tokens"] {
        let mut malformed = report.clone();
        malformed[key][0] = json!("not an object");
        assert_eq!(
            project(json!({}), "v".into(), malformed, 1)
                .unwrap_err()
                .code(),
            "network_result_invalid"
        );
    }
}

#[test]
fn local_argument_and_file_refusals_precede_engine_capture() {
    for (args, code) in [
        (vec![], "target_required"),
        (
            vec!["--model", "local-fixture", "--package", "missing.dsgrid"],
            "target_required",
        ),
        (vec!["--limit", "0"], "invalid_limit"),
        (vec!["--limit", "5001"], "invalid_limit"),
        (vec!["--limit", "one"], "invalid_limit"),
    ] {
        let error = run_with_capture(&inputs(&args), || panic!("refusal must precede capture"))
            .unwrap_err();
        assert_eq!(error.code(), code);
    }
    let root = tempfile::tempdir().unwrap();
    let absent = root.path().join("absent.dsgrid");
    assert_eq!(
        run_with_capture(
            &inputs(&["--package", absent.to_str().unwrap()]),
            || panic!("no package")
        )
        .unwrap_err()
        .code(),
        "model_not_found"
    );
    assert_eq!(COMMAND.effect, Effect::ReadOnly);
    assert_eq!(COMMAND.authority, Authority::None);
    for code in [
        "network_engine_missing",
        "network_result_invalid",
        "package_decode_failed",
        "invalid_limit",
    ] {
        assert!(COMMAND.refusals.iter().any(|refusal| refusal.code == code));
    }
}

#[test]
#[ignore = "requires the explicitly selected existing compatible Network executable"]
fn real_engine_preserves_old_owner_bytes_and_render_for_empty_full_and_limited_reports() {
    assert!(
        Path::new(&std::env::var_os("DS_NETWORK_ENGINE_BIN").expect("explicit existing engine"))
            .is_file()
    );
    let root = tempfile::tempdir().unwrap();
    for rich in [false, true] {
        let path = root
            .path()
            .join(if rich { "rich.dsgrid" } else { "empty.dsgrid" });
        let bytes = fixture(rich);
        fs::write(&path, &bytes).unwrap();
        for limit in [None, Some("1"), Some("2"), Some("5000")] {
            let mut args = vec!["--package", path.to_str().unwrap()];
            if let Some(limit) = limit {
                args.extend(["--limit", limit]);
            }
            let input = inputs(&args);
            let old = old_owner_output(&input, &context()).unwrap();
            let actual = run(&input, &context()).unwrap();
            assert_eq!(
                serde_json::to_vec(&actual).unwrap(),
                serde_json::to_vec(&old).unwrap()
            );
            assert_eq!(render(&actual), render(&old));
            assert_eq!(actual["staged"], false);
            assert_eq!(actual["persisted"], false);
            assert_eq!(fs::read(&path).unwrap(), bytes);
        }
    }
    let damaged = root.path().join("damaged.dsgrid");
    fs::write(&damaged, b"not a package").unwrap();
    let input = inputs(&["--package", damaged.to_str().unwrap()]);
    let old = old_owner_output(&input, &context()).unwrap_err();
    let actual = run(&input, &context()).unwrap_err();
    assert_eq!(actual.code(), old.code());
    assert_eq!(actual.code(), "package_decode_failed");
}

fn isolated(mode: &str) {
    let root = tempfile::tempdir().unwrap();
    let mut command = std::process::Command::new(std::env::current_exe().unwrap());
    command
        .args([
            "--exact",
            "feature_codes::report::tests::isolated_report",
            "--ignored",
            "--nocapture",
            "--test-threads=1",
        ])
        .env("DS_FEATURE_REPORT_TEST_MODE", mode)
        .env("DS_LAYER_HOME", root.path())
        .env("DS_FEATURE_REPORT_TEST_ROOT", root.path())
        .env("TMPDIR", root.path());
    if mode == "absent" {
        command
            .env("HOME", root.path())
            .env_remove("DS_NETWORK_ENGINE_BIN");
    }
    let output = command.output().unwrap();
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    print!("{}", String::from_utf8_lossy(&output.stdout));
}

#[test]
#[cfg(unix)]
fn absent_managed_network_refuses_a_valid_report_without_path_or_linked_fallback() {
    isolated("absent");
}

#[test]
#[ignore = "requires an explicitly selected compatible Network executable"]
fn real_engine_preserves_local_catalogue_authority() {
    isolated("local");
}

#[test]
#[cfg(unix)]
#[ignore = "requires the existing managed Canary Network without the new rich-report capability"]
fn old_managed_engine_refuses_before_query_dispatch() {
    isolated("old");
}

#[test]
#[ignore = "internal subprocess; HOME and local catalogue are isolated by the parent test"]
fn isolated_report() {
    use ds_command_kernel::local_models::{Op, Origin, Scope};
    use ds_layer_store::local_models::{execute_at, package_path, read_at, scope_dir};
    let mode = std::env::var("DS_FEATURE_REPORT_TEST_MODE").expect("internal isolated test");
    let root = std::path::PathBuf::from(std::env::var_os("DS_FEATURE_REPORT_TEST_ROOT").unwrap());
    let bytes = fixture(true);
    let path = root.join("fixture.dsgrid");
    fs::write(&path, &bytes).unwrap();
    let workers = std::thread::available_parallelism().map_or(1, usize::from);
    let capture = || {
        CapturedNetworkClient::capture_for_lane("canary", workers, &|| false)
            .map_err(native::map_failure)
    };
    if mode == "absent" {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let fallback = root.join("ds-network");
            fs::write(&fallback, b"#!/bin/sh\nprintf called > \"$DS_FEATURE_REPORT_TEST_ROOT/fallback-called\"\nexit 97\n").unwrap();
            fs::set_permissions(&fallback, fs::Permissions::from_mode(0o700)).unwrap();
            unsafe {
                std::env::set_var("PATH", &root);
            }
        }
        let error =
            run_with_capture(&inputs(&["--package", path.to_str().unwrap()]), capture).unwrap_err();
        assert_eq!(error.code(), "network_engine_missing");
        assert!(!root.join("fallback-called").exists());
        // The actual public adapter must also refuse an absent explicit selection,
        // even though its source-test build otherwise permits local PATH lookup.
        unsafe {
            std::env::set_var("DS_NETWORK_ENGINE_BIN", root.join("absent-network"));
        }
        assert_eq!(
            run(&inputs(&["--package", path.to_str().unwrap()]), &context())
                .unwrap_err()
                .code(),
            "network_engine_missing"
        );
        assert!(!root.join("fallback-called").exists());
        assert_eq!(fs::read(&path).unwrap(), bytes);
        println!(
            "valid package + absent managed Canary or explicit engine: network_engine_missing; zero fallback calls"
        );
        return;
    }
    if mode == "old" {
        // Exercise the existing managed selection without modifying its registry.
        unsafe {
            std::env::remove_var("DS_NETWORK_ENGINE_BIN");
        }
        let captured = capture().unwrap();
        assert!(!captured.development_override());
        assert!(
            !captured
                .identity()
                .capabilities
                .contains(&NetworkCapability::GridFeatureCodeTableReportV1)
        );
        let old_executable = captured.executable().to_owned();
        println!(
            "old managed Network {} sha256:{}",
            old_executable.display(),
            captured.executable_sha256()
        );
        let input = inputs(&["--package", path.to_str().unwrap()]);
        assert_eq!(
            run_with_capture(&input, || Ok(captured))
                .unwrap_err()
                .code(),
            "network_engine_incompatible"
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let wrapper = root.join("observed-old-network");
            let quote =
                |path: &Path| format!("'{}'", path.display().to_string().replace('\'', "'\\''"));
            fs::write(
                &wrapper,
                format!(
                    "#!/bin/sh\nprintf '%s\\n' \"$1\" >> {}\nexec {} \"$@\"\n",
                    quote(&root.join("engine-calls")),
                    quote(&old_executable)
                ),
            )
            .unwrap();
            fs::set_permissions(&wrapper, fs::Permissions::from_mode(0o700)).unwrap();
            unsafe {
                std::env::set_var("DS_NETWORK_ENGINE_BIN", &wrapper);
            }
            assert_eq!(
                run(&input, &context()).unwrap_err().code(),
                "network_engine_incompatible"
            );
            assert_eq!(
                fs::read_to_string(root.join("engine-calls")).unwrap(),
                "build-info\n"
            );
        }
        assert_eq!(fs::read(&path).unwrap(), bytes);
        println!("missing exact capability: refused after build-info; zero grid-query calls");
        return;
    }
    assert_eq!(mode, "local");
    assert!(
        Path::new(
            &std::env::var_os("DS_NETWORK_ENGINE_BIN").expect("explicit source-captured engine")
        )
        .is_file()
    );
    let capture = native::capture;
    let scope = Scope {
        lane: "stable".into(),
        uid: "report-account-a".into(),
    };
    let register = || Op::Register {
        id: "local-report-fixture".into(),
        display_name: "Owned fixture".into(),
        origin: Origin::Imported,
        crs: "EPSG:32735".into(),
        model_revision: 7,
        bytes: bytes.len() as u64,
        sha256: format!("{:x}", Sha256::digest(&bytes)),
        created_at: None,
        project: None,
        head_revision: None,
        activate: true,
    };
    execute_at(&root, &scope, register(), Some(&bytes)).unwrap();
    let before = read_at(&root, &scope).unwrap();
    let local = inputs(&[
        "--model",
        "local-report-fixture",
        "--account",
        "report-account-a",
        "--lane",
        "stable",
        "--limit",
        "2",
    ]);
    let old = old_owner_output(&local, &context()).unwrap();
    let actual = run_with_capture(&local, capture).unwrap();
    assert_eq!(
        serde_json::to_vec(&actual).unwrap(),
        serde_json::to_vec(&old).unwrap()
    );
    assert_eq!(read_at(&root, &scope).unwrap(), before);
    let held = package_path(&scope_dir(&root, &scope).unwrap(), "local-report-fixture").unwrap();
    assert_eq!(fs::read(held).unwrap(), bytes);
    let other = Scope {
        lane: "stable".into(),
        uid: "report-account-b".into(),
    };
    execute_at(&root, &other, register(), Some(&bytes)).unwrap();
    for (args, code) in [
        (
            vec!["--model", "local-report-fixture", "--lane", "stable"],
            "local_model_ambiguous",
        ),
        (
            vec![
                "--model",
                "local-report-fixture",
                "--account",
                "missing-account",
                "--lane",
                "stable",
            ],
            "local_model_not_found",
        ),
        (
            vec![
                "--model",
                "local-report-fixture",
                "--account",
                "report-account-a",
                "--lane",
                "canary",
            ],
            "local_model_not_found",
        ),
    ] {
        assert_eq!(
            run_with_capture(&inputs(&args), || panic!(
                "local authority refuses before engine"
            ))
            .unwrap_err()
            .code(),
            code
        );
    }
    println!(
        "local catalogue: account/lane/ambiguity preserved; report changes no package or catalogue"
    );
}

// Former production adapter retained only as the independently computed oracle.
fn old_owner_output(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let limit = package::parse_limit(inputs.value("limit"))?;
    let (target, opened) = crate::linked_oracle::open_read_only(inputs)?;
    let report = ds_grid_engine::feature_code_report(opened.session.snapshot());
    let (codes, codes_withheld) = package::take(report.codes.clone(), limit);
    let (unresolved, unresolved_withheld) = package::take(report.unresolved_tokens.clone(), limit);
    let mut answer = json!({
        "target": target,
        "engine": ds_grid_engine::ENGINE_VERSION,
        "code_count": report.code_count,
        "with_clearance_count": report.with_clearance_count,
        "assumed_count": report.assumed_count,
        "voltage_classes": report.voltage_classes,
        "namespaces": report.namespaces,
        "resolved_token_count": report.resolved_token_count,
        "unresolved_token_count": report.unresolved_token_count,
        "unresolved_row_count": report.unresolved_row_count,
        "unknown_code_rows": report.unknown_code_rows,
        "terrain_point_count": report.terrain_point_count,
        "breakline_count": report.breakline_count,
        "crossing_count": report.crossing_count,
        "codes": codes,
        "unresolved_tokens": unresolved,
        "staged": false,
        "persisted": false,
    });
    let mut truncated = Vec::new();
    if codes_withheld > 0 {
        truncated.push(json!({ "field": "codes", "total": report.code_count, "shown": limit, "withheld": codes_withheld, "limit": limit }));
    }
    if unresolved_withheld > 0 {
        truncated.push(json!({ "field": "unresolved_tokens", "total": report.unresolved_token_count, "shown": limit, "withheld": unresolved_withheld, "limit": limit }));
    }
    if !truncated.is_empty() {
        answer["more"] = json!({ "truncated": truncated });
    }
    Ok(answer)
}
