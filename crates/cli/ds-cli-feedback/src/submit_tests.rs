use super::*;

#[test]
fn evidence_and_context_are_bounded() {
    let evidence = vec!["capabilities matched 0".to_string()];
    assert_eq!(
        parse_evidence(&evidence).unwrap(),
        ["capabilities matched 0"]
    );
    let context = vec!["operation=feeder.export".to_string()];
    assert_eq!(
        parse_context(&context).unwrap().get("operation"),
        Some(&"feeder.export")
    );
    assert!(parse_context(&["operation=a".into(), "operation=b".into()]).is_err());
}

#[test]
fn the_report_carries_only_report_fields() {
    // What the command sends is now one typed native command rather than a
    // declared bridge argument list, so the fields are asserted where they
    // are built: nothing about a host, a window or a descriptor may ride
    // along with an operator's report.
    for arg in COMMAND.args {
        assert!(
            !["target", "desktop-descriptor"].contains(&arg.name),
            "`{}` still offers a host flag",
            arg.name
        );
    }
    let declared: Vec<&str> = COMMAND.args.iter().map(|arg| arg.name).collect();
    for field in [
        "title",
        "detail",
        "component",
        "kind",
        "severity",
        "agent",
        "model",
        "client",
        "evidence",
        "context",
    ] {
        assert!(
            declared.contains(&field),
            "`--{field}` is no longer declared"
        );
    }
}

/// The replay travels under one reserved context key: a caller's own
/// `--context replay=` is refused, so a report's replay is always the one ds
/// built from the descriptor and journalled.
#[test]
fn the_replay_context_key_is_reserved_for_replay_last() {
    let refused = parse_context(&["replay={\"argv\":[]}".into()]).unwrap_err();
    assert_eq!(refused.code(), "invalid_context");
    assert!(parse_context(&["replayed=no".into()]).is_ok());
    assert!(
        COMMAND
            .args
            .iter()
            .any(|arg| arg.name == "replay-last"
                && arg.kind == ds_cli_contract::spec::ArgKind::Switch)
    );
    for code in [crate::NO_REPLAY.code, crate::JOURNAL_UNAVAILABLE.code] {
        assert!(
            COMMAND.refusals.iter().any(|refusal| refusal.code == code),
            "{code}"
        );
    }
}
