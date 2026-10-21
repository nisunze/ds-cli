use super::*;
use ds_command_kernel::report_export::input_base_fingerprint;

const SHEETS: &str = "1111111111111111111111111111111111111111111111111111111111111111";
const ENGINE: &str = "2222222222222222222222222222222222222222222222222222222222222222";
const BYTES: &str = "3333333333333333333333333333333333333333333333333333333333333333";
const REFERENCE: &str = "4444444444444444444444444444444444444444444444444444444444444444";
const OTHER_ENGINE: &str = "5555555555555555555555555555555555555555555555555555555555555555";

fn receipt() -> InputReceipt {
    InputReceipt {
        local_print_recipe: None,
        schema: 1,
        country: "RW".into(),
        sheets_json: "{}".into(),
        sheets_sha256: SHEETS.into(),
        reference_semantic_sha256: REFERENCE.into(),
    }
}

fn ids(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| value.to_string()).collect()
}

/// A status row as ds-brain returns it: head `head`, with xlsx/shp/kmz
/// produced by ENGINE from revision `produced_at`, and no pdf.
fn row(name: &str, head: i64, produced_at: i64) -> Value {
    let base = input_base_fingerprint(name, produced_at, SHEETS, "RW", REFERENCE).unwrap();
    let artifact = |id: &str| {
        json!({"output_id": id, "sha256": BYTES, "origin": {
            "work_id": "work-1",
            "input_base_fingerprint": base,
            "engine_build_manifest_sha256": ENGINE,
        }})
    };
    json!({
        "name": name,
        "metadata": {"version": head},
        "report_artifacts": [artifact("xlsx"), artifact("shp"), artifact("kmz")],
    })
}

fn rows(entries: &[(&str, Value)]) -> BTreeMap<String, Value> {
    entries
        .iter()
        .map(|(name, row)| (name.to_string(), row.clone()))
        .collect()
}

#[test]
fn a_current_head_runs_only_the_missing_print_and_a_moved_one_runs_everything() {
    let policy = ids(&["xlsx", "shp", "kmz", "pdf__a3_sheet"]);
    let status = rows(&[("tr_a", row("tr_a", 7, 7)), ("tr_b", row("tr_b", 8, 7))]);
    let (plans, unplanned) = plan_reuse(
        &ids(&["tr_a", "tr_b"]),
        &status,
        &policy,
        &receipt(),
        ENGINE,
    );
    assert!(unplanned.is_empty(), "{unplanned:?}");
    let RunScope::Subset(selection) = &plans["tr_a"].scope else {
        panic!("tr_a holds current data outputs and lacks its print");
    };
    assert_eq!(selection.tokens().unwrap(), ["pdf__a3_sheet"]);
    // tr_b's copies came from revision 7 and its head is 8.
    assert_eq!(plans["tr_b"].scope, RunScope::Full);

    // The fetched room decides: the planned revision runs the subset, a
    // room that moved since the status read runs the full policy.
    assert_eq!(
        run_selection(plans.get("tr_a"), 7),
        (Some(selection.clone()), false)
    );
    assert_eq!(run_selection(plans.get("tr_a"), 8), (None, true));
    assert_eq!(run_selection(plans.get("tr_b"), 8), (None, false));

    let moved = BTreeMap::from([("tr_a".to_string(), 8)]);
    let current = reuse_receipt(None, None, &plans, &unplanned, &BTreeMap::new());
    assert_eq!(current["mode"], "incremental");
    assert_eq!(current["transformers"][0]["scope"], "subset");
    assert_eq!(
        current["transformers"][0]["reused"][0],
        json!({"output_id": "xlsx", "sha256": BYTES, "work_id": "work-1"})
    );
    assert_eq!(
        current["transformers"][0]["generate"],
        json!({"print_freshness_unproven": ["pdf__a3_sheet"]})
    );
    assert_eq!(
        current["transformers"][1]["generate"]["inputs_changed"],
        json!(["xlsx", "shp", "kmz"])
    );
    let fell_back = reuse_receipt(None, None, &plans, &unplanned, &moved);
    assert_eq!(fell_back["transformers"][0]["scope"], "full_room_moved");
    assert_eq!(fell_back["transformers"][0]["reused"], json!([]));
    assert_eq!(fell_back["transformers"][0]["room_version"], 8);
}

#[test]
fn a_printless_policy_can_be_wholly_reused_but_never_across_an_engine_build() {
    let policy = ids(&["xlsx", "shp", "kmz"]);
    let status = rows(&[("tr_a", row("tr_a", 7, 7))]);
    let (same, _) = plan_reuse(&ids(&["tr_a"]), &status, &policy, &receipt(), ENGINE);
    assert_eq!(same["tr_a"].scope, RunScope::AllReused);
    let (moved, _) = plan_reuse(&ids(&["tr_a"]), &status, &policy, &receipt(), OTHER_ENGINE);
    assert_eq!(moved["tr_a"].scope, RunScope::Full);
}

#[test]
fn an_all_reused_claim_is_withdrawn_when_the_confirming_read_no_longer_proves_it() {
    let policy = ids(&["xlsx", "shp", "kmz"]);
    let planned_from = rows(&[("tr_a", row("tr_a", 7, 7)), ("tr_b", row("tr_b", 7, 7))]);
    let (plans, unplanned) = plan_reuse(
        &ids(&["tr_a", "tr_b"]),
        &planned_from,
        &policy,
        &receipt(),
        ENGINE,
    );
    assert!(unplanned.is_empty(), "{unplanned:?}");
    assert!(plans.values().all(|plan| plan.scope == RunScope::AllReused));
    let confirm = |confirming: Option<&BTreeMap<String, Value>>, engine: &str| {
        reconfirm_all_reused(&plans, confirming, &policy, &receipt(), engine)
    };

    // Nothing moved between the plan and the confirmation: every claim stands.
    assert!(confirm(Some(&planned_from), ENGINE).is_empty());

    // A save landed on tr_a after the plan: its head is 8, its copies
    // still come from 7, so tr_a is regenerated and tr_b stays reused.
    let saved = rows(&[("tr_a", row("tr_a", 8, 7)), ("tr_b", row("tr_b", 7, 7))]);
    let withdrawn = confirm(Some(&saved), ENGINE);
    assert_eq!(withdrawn.keys().collect::<Vec<_>>(), ["tr_a"]);
    assert_eq!(
        withdrawn["tr_a"],
        "reuse withdrawn at confirmation: head revision 7 became 8"
    );

    // Same head, but another run replaced tr_b's xlsx copy: the plan's
    // bytes are no longer the project's copy.
    let mut replaced = planned_from.clone();
    replaced.get_mut("tr_b").unwrap()["report_artifacts"][0]["origin"]["work_id"] = json!("work-2");
    let withdrawn = confirm(Some(&replaced), ENGINE);
    assert_eq!(withdrawn.keys().collect::<Vec<_>>(), ["tr_b"]);
    assert!(withdrawn["tr_b"].ends_with("xlsx's copy was replaced"));

    // The engine was replaced after the plan: no copy is current for it.
    let withdrawn = confirm(Some(&planned_from), OTHER_ENGINE);
    assert_eq!(withdrawn.len(), 2);
    assert!(withdrawn["tr_a"].ends_with("xlsx must now be generated (engine_changed)"));

    // A row gone from the confirming read, or no usable read at all
    // (unreachable, refused, another account or project): withdrawn.
    let missing = rows(&[("tr_a", row("tr_a", 7, 7))]);
    let withdrawn = confirm(Some(&missing), ENGINE);
    assert_eq!(withdrawn.keys().collect::<Vec<_>>(), ["tr_b"]);
    assert!(withdrawn["tr_b"].ends_with("the status read returned no row"));
    assert_eq!(confirm(None, ENGINE).len(), 2);

    // A withdrawn transformer leaves the plan and runs the full policy.
    let withdrawn = confirm(Some(&saved), ENGINE);
    let mut plans = plans;
    for name in withdrawn.keys() {
        plans.remove(name);
    }
    assert_eq!(run_selection(plans.get("tr_a"), 8), (None, false));
    assert_eq!(plans["tr_b"].scope, RunScope::AllReused);
}

#[test]
fn no_row_no_saved_head_or_a_malformed_row_is_no_evidence() {
    let status = rows(&[
        ("tr_unsaved", json!({"name": "tr_unsaved", "metadata": {}})),
        (
            "tr_bad",
            json!({"name": "tr_bad", "metadata": {"version": 7}, "report_artifacts": {"xlsx": {}}}),
        ),
    ]);
    let (plans, unplanned) = plan_reuse(
        &ids(&["tr_missing", "tr_unsaved", "tr_bad"]),
        &status,
        &ids(&["xlsx"]),
        &receipt(),
        ENGINE,
    );
    assert!(plans.is_empty());
    assert_eq!(unplanned.len(), 3);
    assert!(unplanned["tr_bad"].starts_with("evidence refused"));
    assert_eq!(run_selection(None, 7), (None, false));
}

#[test]
fn reuse_is_consulted_only_by_a_connected_publishing_run_of_the_same_publisher() {
    assert_eq!(reuse_withheld(true, false, false, true, true), None);
    assert_eq!(
        reuse_withheld(true, true, false, true, true),
        Some("forced")
    );
    assert_eq!(
        reuse_withheld(false, false, false, true, true),
        Some("run_publishes_nothing")
    );
    assert_eq!(
        reuse_withheld(true, false, true, true, true),
        Some("survey_append_freshness_unproven")
    );
    assert_eq!(
        reuse_withheld(true, false, false, false, true),
        Some("status_unread")
    );
    assert_eq!(
        reuse_withheld(true, false, false, true, false),
        Some("publisher_identity_changed")
    );
    let forced = reuse_receipt(
        Some("forced"),
        None,
        &BTreeMap::new(),
        &BTreeMap::new(),
        &BTreeMap::new(),
    );
    assert_eq!(forced["mode"], "forced");
    assert_eq!(forced["transformers"], json!([]));
}
