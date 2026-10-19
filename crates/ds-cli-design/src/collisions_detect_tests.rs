use super::*;

#[test]
fn project_documents_are_never_collision_inputs() {
    for name in [
        "collisions",
        "collisions_2",
        "collisions_17",
        "combined_transformer",
        "all_transformers",
        "mv_data",
        "",
    ] {
        assert!(!collision_input(name), "{name}");
    }
    for name in ["tx_1", "collisions_lv_lines", "kigali_north"] {
        assert!(collision_input(name), "{name}");
    }
}

#[test]
fn the_flags_parse_and_refuse_outside_their_contract() {
    let parsed = ds_cli_contract::parse(
        &DETECT,
        &[
            "--project".into(),
            "p1".into(),
            "--read".into(),
            "held".into(),
        ],
    )
    .unwrap();
    assert_eq!(parsed.value("read"), Some("held"));
    assert_eq!(parsed.value("lane"), Some("stable"));
    assert!(
        ds_cli_contract::parse(
            &DETECT,
            &[
                "--project".into(),
                "p1".into(),
                "--read".into(),
                "cloud".into()
            ]
        )
        .is_err()
    );
    assert!(ds_cli_contract::parse(&PUBLISH, &["--project".into(), "p1".into()]).is_err());
    assert!(DETECT.effect == Effect::LocalFileWrite && PUBLISH.effect == Effect::GlobalWrite);
    assert!(PUBLISH.requires_confirmation() && !DETECT.requires_confirmation());
}

fn result_document() -> Value {
    json!({
        "schema": RESULT_SCHEMA,
        "lane": "canary", "project": "p1", "principal": "uid-1",
        "computed_at": "2026-10-19T12:00:00Z",
        "engine": {"service": "ds-network-reporter", "source_sha": "development", "engine_version": "x"},
        "request_sha256": "a".repeat(64),
        "basis": [{"name": "tx_a", "version": 3}, {"name": "tx_b", "version": null}],
        "input_errors": [{"transformer": "tx_c", "error_message": "report_room_not_held: offline"}],
        "acquisition": {"reused": 2, "rooms_fetched": 0, "heads": "held"},
        "inputs": {"transformers": 2, "features": {"lv_lines": 4}},
        "layers": {"collisions": {"type": "FeatureCollection", "features": [{"type": "Feature"}]}},
        "outcome": {"stage": "collision_detection", "status": "success", "regions": 1, "underlying_pairs": 1}
    })
}

#[test]
fn a_result_document_carries_exactly_the_publication_and_a_bounded_summary() {
    let document = result_document();
    let carried = publication(&document).unwrap();
    carried.validate().unwrap();
    assert_eq!(carried.basis.len(), 2);
    assert_eq!(carried.basis[1].version, None);
    assert_eq!(carried.input_errors[0].transformer, "tx_c");
    let summary = summary(&document);
    assert_eq!(summary["regions"], 1);
    assert_eq!(summary["transformers"], 2);
    assert_eq!(summary["layers"]["collisions"], 1);
    assert_eq!(summary["input_errors"]["count"], 1);
    assert_eq!(summary["published"], false);
    assert!(summary.get("basis").is_none() && summary["layers"]["collisions"].is_number());

    let mut foreign = document;
    foreign["schema"] = json!("ds.design-status-file/v1");
    assert_eq!(
        publication(&foreign).unwrap_err().code(),
        RESULT_INVALID.code
    );
}
