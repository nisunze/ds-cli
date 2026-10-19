use super::*;

#[test]
fn findings_list_every_coded_warning_the_process_recorded() {
    let warning = |code: &str, count: u64, message: &str| json!({"code": code, "count": count, "severity": "warning", "layer": "lv_poles", "message": message});
    let list = TransformerStatusList::from_observation(json!([{
            "name": "upgrade_t",
            "process_metadata": {
                "status": "warning",
                "warning": warning("unconnected_customers", 3, "3 customer(s) could not be connected"),
                "warnings": [
                    warning("unconnected_customers", 3, "3 customer(s) could not be connected"),
                    warning("struct_type_material_unresolved", 1, "Pole material is missing for: C850."),
                    warning("pole_engineering_attributes_missing", 90, "90 pole(s) have no recorded engineering value"),
                    json!({"code": "welded_lv_lines", "count": 6, "severity": "info"})
                ]
            }
        }]))
        .unwrap();

    let data = status_json(&list, true);
    let findings = data["findings"].as_array().unwrap();
    assert_eq!(
        findings
            .iter()
            .map(|finding| (
                finding["transformer"].as_str().unwrap(),
                finding["code"].as_str().unwrap()
            ))
            .collect::<Vec<_>>(),
        vec![
            ("upgrade_t", "unconnected_customers"),
            ("upgrade_t", "struct_type_material_unresolved"),
            ("upgrade_t", "pole_engineering_attributes_missing"),
        ]
    );
    assert_eq!(
        findings[1]["message"],
        "Pole material is missing for: C850."
    );
    assert_eq!(findings[2]["count"], 90);
    assert_eq!(data["transformers"][0]["health"]["warning_count"], 3);
}

#[test]
fn explicit_read_modes_are_additive_and_closed() {
    let parse = |extra: &[&str]| {
        let mut args = vec!["--project".to_string(), "p_one".to_string()];
        args.extend(extra.iter().map(|s| s.to_string()));
        ds_cli_contract::args::parse(&COMMAND, &args)
    };
    assert!(parse(&[]).unwrap().value("read").is_none());
    assert_eq!(
        parse(&["--read", "held"]).unwrap().value("read"),
        Some("held")
    );
    assert_eq!(
        parse(&["--read", "refresh"]).unwrap().value("read"),
        Some("refresh")
    );
    assert!(parse(&["--read", "auto"]).is_err());
}
#[cfg(unix)]
#[test]
fn output_requires_a_private_parent_and_never_replaces_existing_files() {
    use std::os::unix::fs::{PermissionsExt, symlink};
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("status.json");
    validate_output(&path).unwrap();
    std::fs::write(&path, b"preserve").unwrap();
    assert!(validate_output(&path).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), b"preserve");
    let alias = dir.path().join("alias");
    symlink(dir.path(), &alias).unwrap();
    assert!(validate_output(&alias.join("new.json")).is_err());
    std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o755)).unwrap();
    assert!(validate_output(&dir.path().join("new.json")).is_err());
}
fn selector(filters: &[&str]) -> Result<Option<Value>, Failure> {
    let mut argv: Vec<String> = vec!["--project".into(), "p_one".into()];
    for filter in filters {
        argv.push("--filter".into());
        argv.push((*filter).to_string());
    }
    let inputs = ds_cli_contract::args::parse(&COMMAND, &argv).expect("the flags parse");
    selector_from_inputs(&inputs)
}

/// The vocabulary is part of the product, not of a document that is not
/// installed beside it.
#[test]
fn the_filter_dimensions_are_published_where_a_caller_reads_them() {
    // The reference is not installed beside the binary, so the vocabulary
    // has to be reachable from the binary itself. The full list rides on
    // the refusal (proven by the next test); the summary tells the caller
    // that, and carries the one semantic a refusal cannot teach: `user`
    // is the row's LATEST attributed actor, not everyone who touched it.
    let summary = COMMAND
        .args
        .iter()
        .find(|arg| arg.name == "filter")
        .expect("--filter")
        .summary;
    assert!(summary.contains("Refused with the list"), "{summary}");
    assert!(summary.contains("latest actor"), "{summary}");
    assert!(!summary.contains("reference"), "{summary}");
}

#[test]
fn an_unknown_dimension_is_answered_with_the_dimensions_that_exist() {
    let failure = selector(&["colour=red"]).expect_err("an unknown dimension is refused");
    assert_eq!(failure.code(), QUERY_INVALID.code);
    for dimension in FILTER_DIMENSIONS {
        assert!(
            failure.message().contains(dimension),
            "{}",
            failure.message()
        );
    }
}

/// `--sort district` is accepted and `--filter district=` is not, so the
/// refusal names the spelling that works instead of leaving the caller to
/// guess that the two vocabularies differ.
#[test]
fn a_sort_key_used_as_a_filter_dimension_is_told_the_filter_spelling() {
    let failure = selector(&["district=Karongi"]).expect_err("district is not a dimension");
    assert!(
        failure.message().contains("admin-district=<value>"),
        "{}",
        failure.message()
    );
}

/// There is one way to process a transformer, so the register has no
/// lane to filter on and no legacy phase to sort or filter by. Both are
/// refused as unknown dimensions, with the vocabulary that does exist.
#[test]
fn the_retired_lane_and_legacy_dimensions_are_unknown() {
    for retired in ["lane=fast", "lane=standard", "legacy=success"] {
        let failure = selector(&[retired]).expect_err(retired);
        assert_eq!(failure.code(), QUERY_INVALID.code);
        assert!(
            failure.message().contains("unknown dimension"),
            "{}",
            failure.message()
        );
    }
    assert!(!FILTER_DIMENSIONS.contains(&"lane"));
    assert!(!FILTER_DIMENSIONS.contains(&"legacy"));
    assert!(!SORT_ARG.choices.contains(&"legacy"));
}

#[test]
fn an_unknown_admin_level_names_the_levels_that_exist() {
    let failure = selector(&["admin-planet=x"]).expect_err("planet is not a level");
    assert!(
        failure
            .message()
            .contains("district, sector, cell, village"),
        "{}",
        failure.message()
    );
}

/// The kernel clears a value no row exhibits because a browser re-renders
/// without it. One call has no second render, so the same clearing must be
/// a refusal that names what the project does hold.
#[test]
fn a_filter_the_kernel_cleared_is_refused_rather_than_answered_with_a_zero() {
    let asked = json!({ "search": "", "user": "someone@example.com" });
    let normalized = json!({ "user": "", "sync": [], "warning_types": [] });
    let options = json!({ "user": ["hassan@example.com", "nisunze@example.com"] });
    let failure = pruned_filter(&asked, &normalized, &options)
        .expect_err("a pruned scalar filter is a refusal");
    assert_eq!(failure.code(), QUERY_INVALID.code);
    assert!(
        failure.message().contains("hassan@example.com"),
        "{}",
        failure.message()
    );
    assert_eq!(failure.detail_value().expect("detail")["dimension"], "user");
}

#[test]
fn a_pruned_list_filter_names_its_dimension_by_the_flag_spelling() {
    let asked = json!({ "search": "", "warning_types": ["dsr_warning_missing_meter"] });
    let normalized = json!({ "warning_types": [], "user": "" });
    let options = json!({ "warning_types": [{ "key": "dsr_warning_stale", "count": 2 }] });
    let failure = pruned_filter(&asked, &normalized, &options).expect_err("pruned");
    assert!(
        failure.message().starts_with("--filter warning-type="),
        "{}",
        failure.message()
    );
    assert!(
        failure.message().contains("dsr_warning_stale"),
        "{}",
        failure.message()
    );
}

#[test]
fn a_filter_the_kernel_kept_is_not_refused() {
    let asked = json!({ "search": "", "user": "nisunze@example.com", "sync": ["unsaved"] });
    let normalized = json!({ "user": "nisunze@example.com", "sync": ["unsaved"] });
    let options = json!({ "user": ["nisunze@example.com"], "sync": ["saved", "unsaved"] });
    assert!(pruned_filter(&asked, &normalized, &options).is_ok());
    // An absent dimension is not a pruned one.
    assert!(pruned_filter(&json!({ "search": "x" }), &normalized, &options).is_ok());
}
