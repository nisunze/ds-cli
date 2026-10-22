// Test-only pre-process adapter: exact response/refusal parity, never a runtime fallback.
use super::*;
use crate::linked_oracle::map_set_error;
use ds_grid_engine::{ClearanceCase, ClearanceReportError, ClearanceReportOptions};

fn old_owner_output(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let limit = package::parse_limit(inputs.value("limit"))?;
    let case = match inputs.value("case").unwrap_or("both") {
        "vertical" => ClearanceCase::Vertical,
        "horizontal" => ClearanceCase::Horizontal,
        _ => ClearanceCase::Both,
    };
    let alignment_ids = match inputs.value("alignment") {
        Some(raw) => vec![AlignmentId::new(raw.trim()).map_err(|error| {
            Failure::invalid("alignment_not_found", error.to_string())
                .remedy("use an alignment id of the model")
        })?],
        None => Vec::new(),
    };
    let criterion_set_id = match inputs.value("criterion-set") {
        Some(raw) => Some(CriterionSetId::new(raw.trim()).map_err(|error| {
            Failure::invalid("criterion_set_not_found", error.to_string())
                .remedy("pass an id from `ds dsgrid criteria show`")
        })?),
        None => None,
    };
    let corridor_half_width_m = match inputs.value("corridor-half-width-m") {
        Some(raw) => Some(
            raw.trim()
                .parse::<f64>()
                .ok()
                .filter(|v| v.is_finite() && *v > 0.0)
                .ok_or_else(|| {
                    Failure::invalid(
                        "invalid_corridor",
                        "--corridor-half-width-m must be a positive number",
                    )
                    .remedy("pass the plan half-width in metres")
                })?,
        ),
        None => None,
    };
    let (target, opened) = crate::linked_oracle::open_read_only(inputs)?;
    let report = opened
        .session
        .clearance_report(&ClearanceReportOptions {
            alignment_ids,
            case,
            criterion_set_id,
            corridor_half_width_m,
        })
        .map_err(old_map_error)?;
    let finding_count = report.finding_count;
    let (findings, withheld) = package::take(report.findings.clone(), limit);
    let mut answer = json!({
        "target": target,
        "engine": ds_grid_engine::ENGINE_VERSION,
        "operation": { "id": ds_grid_engine::clearance_report::CLEARANCE_REPORT_OPERATION_ID, "schema_version": report.schema_version },
        "classification": report.classification,
        "verification_level": report.verification_level,
        "criterion_set_id": report.criterion_set_id,
        "clearance_voltage_kv": report.clearance_voltage_kv,
        "vertical_case": report.vertical_case,
        "horizontal_case": report.horizontal_case,
        "evaluated_vertical_cases": report.evaluated_vertical_cases,
        "evaluated_horizontal_cases": report.evaluated_horizontal_cases,
        "corridor_half_width_m": report.corridor_half_width_m,
        "definitions": report.definitions,
        "alignments": report.alignments,
        "per_code": report.per_code,
        "points_checked": report.points_checked,
        "points_unresolved": report.points_unresolved,
        "points_outside_spans": report.points_outside_spans,
        "vertical_violations": report.vertical_violations,
        "horizontal_violations": report.horizontal_violations,
        "questionable": report.questionable,
        "finding_count": finding_count,
        "findings": findings,
        "warnings": report.warnings,
        "sections_evaluated": report.sections_evaluated,
        "sections_unavailable": report.sections_unavailable,
        "staged": false,
        "persisted": false,
    });
    if withheld > 0 {
        answer["more"] = json!({ "truncated": [{ "field": "findings", "total": finding_count, "shown": limit, "withheld": withheld, "limit": limit }] });
    }
    Ok(answer)
}

fn old_map_error(error: ClearanceReportError) -> Failure {
    match error {
        ClearanceReportError::CriterionSet(error) => map_set_error(error),
        ClearanceReportError::ClearanceCriteriaNotConfigured { criterion_set } => {
            Failure::invalid(
                "clearance_criteria_not_configured",
                format!("criterion set {criterion_set} has no survey-point clearance cases"),
            )
            .remedy("run `ds dsgrid criteria clearance set --voltage-class MV --vertical-case <label> --horizontal-case <label> --yes` first")
            .next("ds dsgrid criteria show")
        }
        error @ ClearanceReportError::BoundCaseReferenceUnresolved { .. } => Failure::invalid(
            "bound_case_reference_unresolved",
            error.to_string(),
        )
        .remedy("restore the missing analysis case or weather state, or update the named criterion-set binding through `ds dsgrid apply`")
        .next("ds dsgrid describe --kind operations --id update_criterion_case_binding")
        .detail(json!({ "refusal": error })),
        ClearanceReportError::AlignmentNotFound(id) => Failure::invalid(
            "alignment_not_found",
            format!("alignment `{id}` is not in the model"),
        )
        .remedy("use an alignment id of the model"),
        ClearanceReportError::AlignmentHasNoRoute(id) => Failure::invalid(
            "alignment_not_found",
            format!("alignment `{id}` has no route"),
        )
        .remedy("use a routed alignment"),
        ClearanceReportError::NoFeatureCodeClearances => Failure::invalid(
            "feature_codes_without_clearances",
            "no feature code carries clearances",
        )
        .remedy("run `ds dsgrid feature-codes import` and `migrate` first"),
    }
}

fn input(args: &[&str]) -> Inputs {
    ds_cli_contract::parse(
        &COMMAND,
        &args.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
    )
    .unwrap()
}
fn context() -> Context {
    Context {
        confirmed: false,
        output: ds_cli_contract::Output::resolve(ds_cli_contract::Format::Json, false, true),
    }
}

fn fixture() -> ds_grid_exchange::package::GridPackage {
    use ds_grid_engine::{GridSession, clearance_criteria::*, feature_code_ops::*};
    let source = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../../ds-network/crates/ds-grid-exchange/tests/fixtures/native-engine-boundary/v1/model.dsgrid"
    ));
    let current = ds_grid_exchange::package_migration::migrate(source).unwrap();
    let mut package = ds_grid_exchange::package::unpack(&current.bytes).unwrap();
    // Derived span pins belong to the original fixture's authored state.
    package.snapshot.spans.clear();
    for point in &mut package.snapshot.terrain_points {
        point.feature_class = "GROUND".into();
    }
    package.snapshot.tension_sections[0].sag =
        ds_grid_model::AuthoredSag::CatenaryConstant { c_m: 1200.0 };
    let mut session = GridSession::open(package.snapshot);
    let standard = ds_grid_engine::FeatureCodeStandard::bundled().unwrap();
    let import = plan_standard_import(
        session.snapshot(),
        &standard,
        &StandardImportOptions {
            voltage_class: "MV".into(),
            merge: false,
        },
    )
    .unwrap();
    session
        .apply_transaction_at_head(
            session.current_revision().revision_id.clone(),
            import
                .commands
                .into_iter()
                .enumerate()
                .map(|(i, command)| (format!("import-{i}"), command))
                .collect(),
        )
        .unwrap();
    let criteria = plan_clearance_set(
        session.snapshot(),
        &standard,
        &ClearanceSetOptions {
            voltage_class: "MV".into(),
            vertical_case: "Every day".into(),
            horizontal_case: "High wind".into(),
            voltage_kv: None,
            criterion_set_id: None,
        },
    )
    .unwrap();
    session
        .apply_transaction_at_head(
            session.current_revision().revision_id.clone(),
            criteria
                .commands
                .into_iter()
                .enumerate()
                .map(|(i, command)| (format!("criteria-{i}"), command))
                .collect(),
        )
        .unwrap();
    package.snapshot = session.snapshot().clone();
    package
}
fn pack(package: &ds_grid_exchange::package::GridPackage) -> Vec<u8> {
    let model = &package.manifest.model;
    ds_grid_exchange::package::pack(
        &package.snapshot,
        &ds_grid_exchange::PackOptions {
            model_id: model.model_id.clone(),
            model_revision: 17,
            presentation: model.presentation.clone(),
            coordinate_system: model.coordinate_system.clone(),
            library_pins: model.library_pins.clone(),
            library_needs: model.library_needs.clone(),
            assets: package.assets.clone(),
            exchange_bindings: package.exchange_bindings.clone(),
        },
    )
    .unwrap()
}
fn assert_refusal(actual: Failure, expected: Failure) {
    assert_eq!(actual.code(), expected.code());
    assert_eq!(actual.class(), expected.class());
    assert_eq!(actual.message(), expected.message());
    assert_eq!(actual.remedy_text(), expected.remedy_text());
    assert_eq!(actual.next_commands(), expected.next_commands());
    assert_eq!(actual.detail_value(), expected.detail_value());
}

#[test]
fn serialized_owner_refusals_preserve_the_existing_cli_vocabulary() {
    use ds_grid_engine::clearance_criteria::ClearanceSetError;
    for error in [
        ClearanceReportError::CriterionSet(ClearanceSetError::CriterionSetMissing),
        ClearanceReportError::CriterionSet(ClearanceSetError::CriterionSetAmbiguous {
            available: vec!["a".into(), "b".into()],
        }),
        ClearanceReportError::CriterionSet(ClearanceSetError::CriterionSetNotFound(
            "missing".into(),
        )),
        ClearanceReportError::ClearanceCriteriaNotConfigured {
            criterion_set: "set".into(),
        },
        ClearanceReportError::BoundCaseReferenceUnresolved {
            criterion_set: "set".into(),
            role: "vertical".into(),
            analysis_case_id: "case".into(),
            reason: "absent weather".into(),
        },
        ClearanceReportError::AlignmentNotFound("absent".into()),
        ClearanceReportError::AlignmentHasNoRoute("unrouted".into()),
        ClearanceReportError::NoFeatureCodeClearances,
    ] {
        let refusal = json!(error);
        let actual = project_refusal(&refusal, &json!({"engine":error.to_string()})).unwrap();
        assert_refusal(actual, old_map_error(error));
    }
    assert!(project_refusal(&json!({"code":"future_owner_refusal"}), &json!({})).is_none());
}

#[test]
fn local_invalid_inputs_refuse_before_capture_and_preserve_read_authority() {
    for (args, code) in [
        (vec![], "target_required"),
        (vec!["--package", "a", "--model", "b"], "target_required"),
        (vec!["--limit", "0"], "invalid_limit"),
        (vec!["--limit", "5001"], "invalid_limit"),
        (vec!["--corridor-half-width-m", "NaN"], "invalid_corridor"),
        (vec!["--corridor-half-width-m", "0"], "invalid_corridor"),
        (vec!["--alignment", " "], "alignment_not_found"),
        (vec!["--criterion-set", " "], "criterion_set_not_found"),
    ] {
        assert_eq!(
            run_with_capture(&input(&args), || panic!("invalid input captured engine"))
                .unwrap_err()
                .code(),
            code
        );
    }
    assert_eq!(COMMAND.effect, Effect::ReadOnly);
    assert_eq!(COMMAND.authority, Authority::None);
    for code in [
        "network_engine_missing",
        "network_result_invalid",
        "package_decode_failed",
    ] {
        assert!(COMMAND.refusals.iter().any(|item| item.code == code));
    }
}

#[test]
fn projection_keeps_complete_owner_evidence_and_refuses_inconsistent_counts() {
    let package = fixture();
    let report = ds_grid_engine::clearance_report(&package.snapshot, &Default::default()).unwrap();
    assert!(report.finding_count > 1, "fixture must exercise truncation");
    let mut value = json!(report);
    value["findings"][0]["native_future_evidence"] = json!({"exact":"keep","number":-0.0});
    let expected = value["findings"][0].clone();
    let answer = project(json!({}), "test-engine".into(), value.clone(), 1).unwrap();
    assert_eq!(answer["findings"][0], expected);
    assert_eq!(
        answer["more"]["truncated"][0]["total"],
        report.finding_count
    );
    assert_eq!(
        answer["sections_unavailable"],
        value["sections_unavailable"]
    );
    value["finding_count"] = json!(report.finding_count + 1);
    assert_eq!(
        project(json!({}), "test".into(), value, 1)
            .unwrap_err()
            .code(),
        "network_result_invalid"
    );
}

#[test]
#[ignore = "requires the explicitly selected compatible Network executable"]
fn real_engine_preserves_clearance_bytes_render_and_typed_refusals() {
    assert!(
        std::path::Path::new(&std::env::var_os("DS_NETWORK_ENGINE_BIN").expect("explicit engine"))
            .is_file()
    );
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("clearance.dsgrid");
    let package = fixture();
    let bytes = pack(&package);
    std::fs::write(&path, &bytes).unwrap();
    for case in ["vertical", "horizontal", "both"] {
        for limit in ["1", "5000"] {
            let args = input(&[
                "--package",
                path.to_str().unwrap(),
                "--case",
                case,
                "--limit",
                limit,
            ]);
            let old = old_owner_output(&args, &context()).unwrap();
            let actual = run(&args, &context()).unwrap();
            assert_eq!(
                serde_json::to_vec(&actual).unwrap(),
                serde_json::to_vec(&old).unwrap()
            );
            assert_eq!(render(&actual), render(&old));
            assert_eq!(std::fs::read(&path).unwrap(), bytes);
        }
    }
    for more in [
        vec!["--alignment", "absent"],
        vec!["--criterion-set", "absent"],
    ] {
        let mut args = vec!["--package", path.to_str().unwrap()];
        args.extend(more);
        let args = input(&args);
        assert_refusal(
            run(&args, &context()).unwrap_err(),
            old_owner_output(&args, &context()).unwrap_err(),
        );
    }
    // Use already-valid packages for owner refusals. Inventing broken table
    // references here would exercise package validation instead of this query.
    let blank = ds_grid_exchange::create_blank_model(&Default::default())
        .unwrap()
        .bytes;
    let unconfigured = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../../ds-network/crates/ds-grid-exchange/tests/fixtures/native-engine-boundary/v1/model.dsgrid"
    ));
    for refused in [blank.as_slice(), unconfigured.as_slice()] {
        std::fs::write(&path, refused).unwrap();
        let args = input(&["--package", path.to_str().unwrap()]);
        assert_refusal(
            run(&args, &context()).unwrap_err(),
            old_owner_output(&args, &context()).unwrap_err(),
        );
        assert_eq!(std::fs::read(&path).unwrap(), refused);
    }
    std::fs::write(&path, b"not a package").unwrap();
    assert_eq!(
        run(&input(&["--package", path.to_str().unwrap()]), &context())
            .unwrap_err()
            .code(),
        "package_decode_failed"
    );
}

#[test]
#[cfg(unix)]
fn missing_engine_refuses_clearance_and_identity_without_fallback() {
    let root = tempfile::tempdir().unwrap();
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "analyse::clearance::process_tests::isolated_missing_engine",
            "--ignored",
            "--nocapture",
        ])
        .env("DS_CLEARANCE_TEST_ROOT", root.path())
        .env("DS_NETWORK_ENGINE_BIN", root.path().join("absent-network"))
        .env("HOME", root.path())
        .env("DS_LAYER_HOME", root.path())
        .env("TMPDIR", root.path())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
#[cfg(unix)]
#[ignore = "internal process with isolated engine/account state"]
fn isolated_missing_engine() {
    use std::os::unix::fs::PermissionsExt;
    let root = std::path::PathBuf::from(
        std::env::var_os("DS_CLEARANCE_TEST_ROOT").expect("isolated test"),
    );
    let fallback = root.join("ds-network");
    std::fs::write(
        &fallback,
        b"#!/bin/sh\nprintf called > \"$DS_CLEARANCE_TEST_ROOT/fallback-called\"\nexit 97\n",
    )
    .unwrap();
    std::fs::set_permissions(&fallback, std::fs::Permissions::from_mode(0o700)).unwrap();
    unsafe {
        std::env::set_var("PATH", &root);
    }
    let bytes = pack(&fixture());
    let path = root.join("valid.dsgrid");
    std::fs::write(&path, &bytes).unwrap();
    assert_eq!(
        run(&input(&["--package", path.to_str().unwrap()]), &context())
            .unwrap_err()
            .code(),
        "network_engine_missing"
    );
    assert_eq!(
        crate::model::workspace::identity(&bytes)
            .err()
            .unwrap()
            .code(),
        "network_engine_missing"
    );
    assert!(!root.join("fallback-called").exists());
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
}
