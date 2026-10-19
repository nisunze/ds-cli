use super::*;
use serde_json::json;

fn health(row: Value) -> TransformerHealth {
    transformer_health(&row)
}

fn codes(health: &TransformerHealth) -> Vec<&str> {
    health
        .findings
        .iter()
        .map(|finding| finding.code.as_str())
        .collect()
}

#[test]
fn a_row_with_no_phase_metadata_is_ok_and_says_nothing() {
    let verdict = health(json!({"name": "tr_1"}));
    assert_eq!(verdict.severity, Severity::Ok);
    assert!(verdict.findings.is_empty());
    assert_eq!(verdict.error_count, 0);
    assert_eq!(verdict.warning_count, 0);
}

#[test]
fn a_phase_error_reads_its_message_from_either_place_it_is_written() {
    let verdict = health(json!({"name": "tr_1",
            "process_metadata": {"status": "error", "error_message": "network build failed"}}));
    assert_eq!(verdict.severity, Severity::Error);
    assert_eq!(verdict.error_count, 1);
    assert_eq!(verdict.findings[0].phase, Phase::Process);
    assert_eq!(verdict.findings[0].label_key, "dsh_phase_error");
    assert_eq!(verdict.findings[0].message, "network build failed");

    let verdict = health(json!({"name": "tr_1",
            "report_metadata": {"status": "error", "error": {"message": "export crashed"}}}));
    assert_eq!(verdict.findings[0].message, "export crashed");

    // A whitespace-only `error_message` is truthy in JavaScript: it wins
    // the choice and then trims to nothing. The nested message is NOT
    // consulted, and that is the behaviour, not a bug to fix here.
    let verdict = health(json!({"name": "tr_1",
            "draft_metadata": {"status": "error", "error_message": "   ",
            "error": {"message": "never read"}}}));
    assert_eq!(verdict.findings[0].message, "");
}

#[test]
fn a_coded_warning_carries_its_label_count_and_where_to_look() {
    let verdict = health(
        json!({"name": "tr_1", "process_metadata": {"status": "warning",
            "warning": {"code": "unconnected_lv_lines", "message": "3 LV lines could not connect", "count": 3}}}),
    );
    assert_eq!(verdict.severity, Severity::Warning);
    assert_eq!(verdict.warning_count, 1);
    let finding = &verdict.findings[0];
    assert_eq!(finding.code, "unconnected_lv_lines");
    assert_eq!(finding.label_key, "dsh_finding_unconnected_lv_lines");
    assert_eq!(finding.count, Some(Number::from(3)));
    assert_eq!(finding.affected, Some(Number::from(3)));
    assert_eq!(
        finding.locatable,
        Some(located_unconnected_lv_lines()),
        "the offending lines are addressable"
    );
    // An integral count is written the way JavaScript writes it.
    assert!(serde_json::to_string(finding)
        .unwrap()
        .contains("\"count\":3"));
}

#[test]
fn every_listed_warning_code_is_a_finding_not_only_the_promoted_one() {
    let verdict = health(json!({"name": "upgrade_t", "process_metadata": {
        "status": "warning",
        "warning": {"code": "unconnected_customers", "count": 12, "severity": "warning",
            "message": "12 customer(s) could not be connected"},
        "warnings": [
            {"code": "unconnected_customers", "count": 12, "severity": "warning",
                "message": "12 customer(s) could not be connected"},
            {"code": "unconnected_lv_poles", "count": 2, "severity": "warning",
                "layer": "unconnected_lv_poles", "message": "2 pole(s) unconnected"},
            {"code": "welded_lv_lines", "count": 32, "severity": "info",
                "message": "welded 32 taps"},
            {"code": "struct_type_material_unresolved", "count": 1, "severity": "warning",
                "layer": "lv_poles", "message": "Pole material is missing for: C850."},
            {"code": "lv_span_exceeds_max_length", "count": 1, "severity": "warning",
                "layer": "spans", "message": "LV span span_1 is 50.06 m"},
            {"code": "lv_span_exceeds_max_length", "count": 1, "severity": "warning",
                "layer": "spans", "message": "LV span span_2 is 51.2 m"},
            {"code": "", "count": 9, "severity": "warning", "message": "uncoded"}
        ]
    }}));

    assert_eq!(verdict.severity, Severity::Warning);
    assert_eq!(
        codes(&verdict),
        vec![
            "unconnected_customers",
            "unconnected_lv_poles",
            "struct_type_material_unresolved",
            "lv_span_exceeds_max_length"
        ],
        "the promoted warning is not repeated and an info note is not a finding"
    );
    assert_eq!(verdict.warning_count, 4);
    let material = &verdict.findings[2];
    assert_eq!(material.phase, Phase::Process);
    assert_eq!(material.source, Source::Document);
    assert_eq!(material.layer.as_deref(), Some("lv_poles"));
    assert_eq!(material.message, "Pole material is missing for: C850.");
    assert_eq!(material.count, Some(Number::from(1)));
    let spans = &verdict.findings[3];
    assert_eq!(
        spans.count,
        Some(Number::from(2)),
        "one finding per code, counts summed"
    );
    assert_eq!(spans.affected, Some(Number::from(2)));
    assert_eq!(spans.message, "LV span span_1 is 50.06 m");
    // A listed diagnostic code also suppresses the same diagnostic-layer
    // finding, so it is never counted twice.
    let verdict = health(json!({"name": "t", "layers": {"unconnected_lv_poles": 2},
            "process_metadata": {"status": "warning",
                "warning": {"code": "service_cable_customer_unresolved", "count": 1},
                "warnings": [
                    {"code": "service_cable_customer_unresolved", "count": 1, "severity": "warning"},
                    {"code": "unconnected_lv_poles", "count": 2, "severity": "warning"}]}}));
    assert_eq!(
        codes(&verdict),
        vec!["service_cable_customer_unresolved", "unconnected_lv_poles"]
    );
    // A run that only left info notes says nothing.
    let verdict = health(
        json!({"name": "t", "process_metadata": {"status": "success",
            "warnings": [{"code": "welded_lv_lines", "count": 3, "severity": "info"}]}}),
    );
    assert!(verdict.findings.is_empty());
    assert_eq!(verdict.severity, Severity::Ok);
}

#[test]
fn a_warning_with_no_count_is_counted_from_its_details() {
    let verdict = health(
        json!({"name": "tr_1", "process_metadata": {"status": "warning",
            "warning": {"code": "lost_nodes", "message": "nodes lost", "details": ["n1", "n2", "n3", "n4"]}}}),
    );
    assert_eq!(verdict.findings[0].count, Some(Number::from(4)));
    assert_eq!(verdict.findings[0].affected, Some(Number::from(4)));
    assert_eq!(verdict.findings[0].label_key, "dsh_finding_lost_nodes");
}

#[test]
fn the_affected_ladder_sums_the_counts_the_details_state() {
    // Branch two: no top-level count, but the entries carry their own —
    // including a numeric STRING, which `Number()` reads and `typeof` does
    // not. `count` still reports how many entries there are.
    let verdict = health(
        json!({"name": "tr_1", "report_metadata": {"status": "warning",
            "warning": {"code": "report_export_partial", "details": [
                {"layer": "customers", "count": 2, "code": "dropped"},
                {"layer": "spans", "count": "3"}]}}}),
    );
    let finding = &verdict.findings[0];
    assert_eq!(finding.count, Some(Number::from(2)));
    assert_eq!(finding.affected, Some(Number::from(5)));
    assert_eq!(
        finding.detail_parts,
        vec!["customers / 2 / dropped", "spans / 3"]
    );
    assert_eq!(finding.detail_total, 2);
    assert_eq!(finding.label_key, "dlbl_warn_report_export_partial");
}

#[test]
fn a_details_preview_is_bounded_and_a_non_list_detail_is_one_string() {
    let entries: Vec<Value> = (0..14)
        .map(|index| json!({"layer": format!("l{index}")}))
        .collect();
    let verdict = health(
        json!({"name": "tr_1", "process_metadata": {"status": "warning",
            "warning": {"code": "lost_nodes", "details": entries}}}),
    );
    assert_eq!(verdict.findings[0].detail_parts.len(), DETAIL_PREVIEW_MAX);
    assert_eq!(verdict.findings[0].detail_total, 14);

    let verdict = health(
        json!({"name": "tr_1", "process_metadata": {"status": "warning",
            "warning": {"code": "lost_nodes", "details": "a".repeat(400)}}}),
    );
    assert_eq!(
        verdict.findings[0].detail_parts[0].chars().count(),
        DETAIL_STRING_MAX_CHARS
    );
    assert_eq!(verdict.findings[0].detail_total, 1);
    // No list, so nothing to count: the ladder reports nothing rather than
    // inventing a number.
    assert_eq!(verdict.findings[0].affected, None);
}

#[test]
fn a_positive_diagnostic_layer_is_the_warning_and_is_never_counted_twice() {
    let verdict = health(
        json!({"name": "tr_1", "process_metadata": {"status": "success"},
            "layers": {"lv_lines": 40, "non_snapping_lv_lines": 5, "lv_poles": 12}}),
    );
    assert_eq!(verdict.severity, Severity::Warning);
    let finding = verdict
        .findings
        .iter()
        .find(|finding| finding.code == "non_snapping_lv_lines")
        .expect("the buried lines are reported");
    assert_eq!(finding.count, Some(Number::from(5)));
    assert_eq!(finding.locatable.as_ref().unwrap().layer, "lv_lines");

    // Supplied as a feature array rather than a count.
    let verdict =
        health(json!({"name": "tr_1", "layers": {"unconnected_customers": [{}, {}, {}]}}));
    assert_eq!(verdict.findings[0].count, Some(Number::from(3)));

    // The phase warning already named it, so the layer does not repeat it.
    let verdict = health(
        json!({"name": "tr_1", "process_metadata": {"status": "warning",
            "warning": {"code": "non_snapping_lv_lines", "message": "buried", "count": 2}},
            "layers": {"non_snapping_lv_lines": 2}}),
    );
    assert_eq!(codes(&verdict), vec!["non_snapping_lv_lines"]);
    assert_eq!(verdict.findings[0].message, "buried");
}

#[test]
fn every_diagnostic_layer_has_a_label_and_a_place_to_look() {
    for (layer, label_key, locatable) in diagnostic_layers() {
        let verdict = health(json!({"name": "tr_1", "layers": {layer: 1}}));
        assert_eq!(codes(&verdict), vec![layer]);
        assert_eq!(verdict.findings[0].label_key, label_key);
        // The directive the table declares is the directive the finding
        // carries — the whole point of naming a place to look.
        assert_eq!(
            verdict.findings[0].locatable.as_ref(),
            Some(&locatable),
            "{layer}"
        );
    }
}

#[test]
fn a_run_that_never_landed_is_stuck_and_one_in_flight_is_not_a_problem() {
    let verdict =
        health(json!({"name": "tr_1", "process_metadata": {"status": "stale_processing"}}));
    assert_eq!(verdict.severity, Severity::Warning);
    assert_eq!(verdict.findings[0].code, "stuck_processing");
    assert_eq!(verdict.findings[0].label_key, "dsh_phase_stuck");

    let verdict = health(
        json!({"name": "tr_1", "process_metadata": {"status": "processing",
            "progress": {"message": "building network"}}}),
    );
    assert_eq!(verdict.severity, Severity::Processing);
    assert_eq!(verdict.error_count + verdict.warning_count, 0);
    assert_eq!(verdict.findings[0].message, "building network");
}

#[test]
fn a_routine_post_save_stale_report_says_nothing_at_all() {
    let verdict = health(
        json!({"name": "tr_1", "report_metadata": {"status": "stale"},
            "combined_inclusion_metadata": {"status": "stale"}}),
    );
    assert_eq!(verdict.severity, Severity::Ok);
    assert!(verdict.findings.is_empty());
    // And so does a legacy phase the supersession pass rewrote to `stale`.
    let verdict = health(json!({"name": "tr_1", "draft_metadata": {"status": "stale",
            "superseded_by": "process", "superseded_error_message": "old failure"}}));
    assert!(verdict.findings.is_empty());
}

#[test]
fn an_outcome_sub_leg_is_surfaced_even_when_its_phase_reads_success() {
    let verdict = health(
        json!({"name": "tr_1", "report_metadata": {"status": "success",
            "outcomes": {"report_export_partial": {"status": "warning", "message": "partial export", "count": 2}}}}),
    );
    assert_eq!(verdict.severity, Severity::Warning);
    assert_eq!(verdict.findings[0].code, "report_export_partial");
    assert_eq!(verdict.findings[0].message, "partial export");
    assert_eq!(verdict.findings[0].count, Some(Number::from(2)));

    let verdict = health(
        json!({"name": "tr_1", "report_metadata": {"status": "success",
            "outcomes": {"survey_form_export": {"status": "error", "error_message": "form export failed"}}}}),
    );
    assert_eq!(verdict.severity, Severity::Error);
    assert_eq!(verdict.findings[0].severity, FindingSeverity::Error);
    assert_eq!(verdict.findings[0].message, "form export failed");

    // A nested warning list under a sub-leg that itself reads success.
    let verdict = health(
        json!({"name": "tr_1", "report_metadata": {"status": "success",
            "outcomes": {"media": {"status": "success",
                "warnings": [{"code": "missing_geometry_dropped", "message": "2 rows", "count": 2},
                             {"severity": "error", "code": "self_loop_dropped"},
                             {"message": "no code, not reported"}]}}}}),
    );
    assert_eq!(
        codes(&verdict),
        vec!["self_loop_dropped", "missing_geometry_dropped"]
    );
    assert_eq!(
        verdict.findings[1].label_key,
        "dlbl_warn_missing_geometry_dropped"
    );

    // A sub-leg with nothing of its own reaches into the legs beneath it.
    let verdict = health(
        json!({"name": "tr_1", "report_metadata": {"status": "success",
            "outcomes": {"export": {"status": "error",
                "errors": [{}, {"technical_detail": "row 4 has no geometry"}]}}}}),
    );
    assert_eq!(verdict.findings[0].message, "row 4 has no geometry");
}

#[test]
fn findings_are_ordered_error_first_and_the_worst_one_is_the_verdict() {
    let verdict = health(json!({"name": "tr_1",
            "process_metadata": {"status": "warning", "warning": {"code": "lost_nodes", "message": "w"}},
            "report_metadata": {"status": "error", "error_message": "boom"},
            "layers": {"drafting_errors": 1}}));
    assert_eq!(verdict.severity, Severity::Error);
    assert_eq!(verdict.findings[0].severity, FindingSeverity::Error);
    assert_eq!(verdict.error_count, 1);
    assert_eq!(verdict.warning_count, 2);
    // Within a severity the phase-slot order stands: the process warning
    // was read before the diagnostic layer.
    assert_eq!(codes(&verdict), vec!["", "lost_nodes", "drafting_errors"]);
}

#[test]
fn a_green_process_badge_that_saved_no_output_is_an_error_and_an_empty_map_is_silence() {
    let verdict = health(
        json!({"name": "tr_1", "process_metadata": {"status": "success"},
            "layers": {"customers": 10, "lv_lines": 4}}),
    );
    assert_eq!(verdict.severity, Severity::Error);
    assert_eq!(codes(&verdict), vec![PROCESSED_OUTPUT_MISSING]);
    assert_eq!(verdict.findings[0].phase, Phase::Process);
    assert!(verdict.findings[0].locatable.is_none());

    for layer in PROCESSED_OUTPUT_LAYERS {
        let verdict = health(
            json!({"name": "tr_1", "process_metadata": {"status": "success"},
                "layers": {layer: 1}}),
        );
        assert!(verdict.findings.is_empty(), "{layer} is output");
    }
    // Older documents carry no layer counts: unknown, so no alarm.
    for row in [
        json!({"name": "tr_1", "process_metadata": {"status": "success"}}),
        json!({"name": "tr_1", "process_metadata": {"status": "success"}, "layers": {}}),
    ] {
        assert!(health(row).findings.is_empty());
    }
    // An aggregate row is not an LV transformer and never persists them.
    let verdict = health(
        json!({"name": "mv_data", "process_metadata": {"status": "success"},
            "layers": {"mv_lines": 10}}),
    );
    assert!(verdict.findings.is_empty());
}

#[test]
fn the_backend_words_are_bounded_and_never_carry_a_credential() {
    let verdict = health(
        json!({"name": "tr_1", "process_metadata": {"status": "error",
            "error_message": "upload to https://storage.googleapis.com/b/o?X-Goog-Signature=deadbeef failed with Bearer ya29.a0AfH6SMBsecret"}}),
    );
    let message = &verdict.findings[0].message;
    assert!(!message.contains("ya29"), "{message}");
    assert!(!message.contains("googleapis"), "{message}");
    assert!(message.contains(REDACTED), "{message}");

    // Platform vocabulary an operator cannot act on becomes product words.
    let verdict = health(
        json!({"name": "tr_1", "process_metadata": {"status": "error",
            "error_message": "the Firestore write failed and IndexedDB is full"}}),
    );
    assert_eq!(
        verdict.findings[0].message,
        "the project database write failed and this browser's local storage is full"
    );

    let long = "x".repeat(MESSAGE_MAX_CHARS + 200);
    let verdict = health(
        json!({"name": "tr_1", "process_metadata": {"status": "error", "error_message": long}}),
    );
    assert_eq!(
        verdict.findings[0].message.chars().count(),
        MESSAGE_MAX_CHARS
    );
}

#[test]
fn the_last_run_speaks_beside_the_document_and_never_over_it() {
    let row = json!({"name": "tr_1", "process_metadata": {"status": "warning",
            "warning": {"code": "lost_nodes", "message": "nodes lost", "count": 2}}});
    let run = json!({"warnings": [
            {"code": "lost_nodes", "message": "nodes lost", "count": 2},
            {"code": "welded_lv_lines", "message": "3 taps noded", "count": 3, "layer": "lv_lines"},
            {"code": "dropped_lv_poles", "severity": "error", "message": "1 pole dropped"}]});
    let verdict = transformer_health_with_run(&row, Some(&run));
    // The document's copy is kept and the run's duplicate is dropped.
    assert_eq!(
        codes(&verdict),
        vec!["lost_nodes", "welded_lv_lines", "dropped_lv_poles"]
    );
    assert_eq!(verdict.findings[0].source, Source::Document);
    assert_eq!(verdict.findings[1].source, Source::Run);
    assert_eq!(verdict.findings[1].layer.as_deref(), Some("lv_lines"));
    assert_eq!(verdict.findings[2].severity, FindingSeverity::Error);
    // The badge still reports what the document says.
    assert_eq!(verdict.severity, Severity::Warning);
    assert_eq!(verdict.error_count, 0);
    assert_eq!(verdict.warning_count, 1);
}

#[test]
fn the_summary_counts_rows_the_way_a_refresh_reports_them() {
    let reply = evaluate(
        &serde_json::to_vec(&json!({"schema": SCHEMA, "rows": [
                {"name": "a", "process_metadata": {"status": "error", "error_message": "boom"},
                 "report_metadata": {"status": "processing"}},
                {"name": "b", "layers": {"drafting_errors": 3}},
                {"name": "c", "process_metadata": {"status": "processing"}},
                {"name": "d"}]}))
        .unwrap(),
    )
    .unwrap();
    let reply: Value = serde_json::from_str(&reply).unwrap();
    assert_eq!(reply["schema"], SCHEMA);
    // A row carrying both an error and an in-flight phase is counted in
    // both buckets, exactly as the refresh feedback has always counted it.
    assert_eq!(
        reply["summary"],
        json!({"total": 4, "processing": 2, "warnings": 1, "errors": 1})
    );
    assert_eq!(reply["rows"][0]["name"], "a");
    assert_eq!(reply["rows"][0]["severity"], "error");
    assert_eq!(reply["rows"][1]["warning_count"], 1);
}

#[test]
fn a_row_is_answered_under_the_exact_name_it_was_sent_under() {
    // The host looks its answer up by the name it holds. A trimmed echo
    // would miss, and the row would silently contribute nothing to the
    // project's finding list while every surface paid for a second call.
    let reply = evaluate(
        &serde_json::to_vec(&json!({"schema": SCHEMA,
                "rows": [{"name": " tr 1 ", "layers": {"drafting_errors": 2}}],
                "run": {" tr 1 ": {"warnings": [{"code": "lost_nodes", "message": "two lost"}]}}}))
        .unwrap(),
    )
    .unwrap();
    let reply: Value = serde_json::from_str(&reply).unwrap();
    assert_eq!(reply["rows"][0]["name"], " tr 1 ");
    // And the run slice, keyed by that same name, still reaches it.
    assert_eq!(reply["rows"][0]["findings"][1]["code"], "lost_nodes");
    assert_eq!(reply["rows"][0]["findings"][1]["source"], "run");
}

#[test]
fn the_door_is_closed_to_anything_it_did_not_declare() {
    assert!(evaluate(br#"{"schema":"other","rows":[]}"#).is_err());
    assert!(evaluate(br#"{"schema":"ds.design-health/v1","rows":[],"now_ms":1}"#).is_err());
    assert!(evaluate(br#"{"schema":"ds.design-health/v1"}"#).is_err());
    assert!(evaluate(&vec![b' '; MAX_INPUT_BYTES + 1]).is_err());
    // A row that is not an object is a row with nothing to say, not a
    // decode failure: ds-brain has been known to send `null` entries.
    let reply = evaluate(br#"{"schema":"ds.design-health/v1","rows":[null,3,"x"]}"#).unwrap();
    let reply: Value = serde_json::from_str(&reply).unwrap();
    assert_eq!(reply["summary"]["total"], 3);
    assert_eq!(reply["rows"][0]["name"], "");
}
