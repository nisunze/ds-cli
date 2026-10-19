use super::*;

/// The keys are the kernel's; this pins that the command reads them from
/// there rather than restating the fold.
#[test]
fn the_state_key_separates_never_checked_from_unknown_and_from_zero() {
    assert_eq!(state_key(false, None), "pctl_collisions_never_checked");
    assert_eq!(state_key(true, None), "pctl_collisions_unknown");
    assert_eq!(state_key(true, Some(0)), "pctl_collisions_pairs_none");
    assert_eq!(state_key(true, Some(1)), "pctl_collisions_pairs_one");
    assert_eq!(state_key(true, Some(7)), "pctl_collisions_pairs_many");
}

#[test]
fn the_project_wide_documents_are_not_transformers_in_scope() {
    for name in ["collisions", "combined_transformer", "mv_data"] {
        assert!(is_reserved(name));
    }
    assert!(!is_reserved("TX-1"));
}

/// The precedence is the kernel's; this pins that the command reads the
/// document through it rather than reaching for one member.
#[test]
fn a_zero_result_document_reports_zero_not_unknown() {
    let row = json!({
        "layers": {},
        "report_metadata": {"outcomes": {"collision_detection": {"details": {"pairs": 0}}}}
    });
    assert_eq!(
        ds_command_kernel::collisions::project_count(&row, &Value::Null),
        Some(0)
    );
}

#[test]
fn removed_region_flags_refuse_before_project_authorization() {
    for removed in [vec!["--regions"], vec!["--limit", "10"]] {
        let mut args = vec!["--project".to_owned(), "fixture-project".to_owned()];
        args.extend(removed.into_iter().map(str::to_owned));
        let error = ds_cli_contract::parse(&COMMAND, &args)
            .err()
            .expect("removed flag refused");
        assert_eq!(error.code(), "unknown_flag");
    }
    let parsed =
        ds_cli_contract::parse(&COMMAND, &["--project".into(), "fixture-project".into()]).unwrap();
    assert_eq!(parsed.value("project"), Some("fixture-project"));
    assert_eq!(parsed.value("lane"), Some("stable"));
}

#[test]
fn count_precedence_and_malformed_values_are_retained_without_regions() {
    let row = json!({"layers":{"collisions":2},"report_metadata":{"outcomes":{"collision_detection":{"details":{"pairs":3,"regions":4}}}}});
    assert_eq!(
        ds_command_kernel::collisions::project_count(&row, &Value::Null),
        Some(2)
    );
    let mut row = row;
    row["layers"]["collisions"] = json!("not a count");
    assert_eq!(
        ds_command_kernel::collisions::project_count(&row, &Value::Null),
        Some(3)
    );
    row["report_metadata"]["outcomes"]["collision_detection"]["details"]["pairs"] = Value::Null;
    assert_eq!(
        ds_command_kernel::collisions::project_count(&row, &Value::Null),
        Some(4)
    );
    row["report_metadata"]["outcomes"]["collision_detection"]["details"]["regions"] = json!("4");
    assert_eq!(
        ds_command_kernel::collisions::project_count(&row, &Value::Null),
        None
    );
}
