use super::*;
use serde_json::json;

fn recipe(layout: Value) -> Value {
    json!({"schema": COMPOSITION_SCHEMA, "transformers": ["tx_a"], "archive_layout": layout, "force": false})
}

fn file(name: &str, byte: char) -> StagedFile {
    StagedFile {
        filename: name.to_owned(),
        source: format!("/stage/{name}"),
        sha256: byte.to_string().repeat(64),
        size_bytes: 10,
    }
}

fn member(name: &str, revision: i64) -> Member {
    Member {
        transformer: name.to_owned(),
        revision,
        batch_id: format!("batch_{name}"),
        files: vec![
            file(&format!("{name}.xlsx"), 'a'),
            file(&format!("{name}.kmz"), 'b'),
        ],
    }
}

fn root_set(names: &[&str]) -> CombinedSet {
    CombinedSet {
        group: None,
        transformers: names.iter().map(|n| (*n).to_owned()).collect(),
        files: vec![file("combined_transformer.xlsx", 'c')],
    }
}

fn plan_of(
    composition: &Composition,
    groups: &GroupPaths,
    combined: &[CombinedSet],
) -> Result<BundlePlan, Refusal> {
    let participating = vec!["tx_a".to_owned(), "tx_b".to_owned()];
    let members = vec![member("tx_a", 3), member("tx_b", 5)];
    plan(&PlanInput {
        project: "p1",
        composition,
        participating: &participating,
        members: &members,
        groups,
        combined,
        output: "/stage/compounded_reports.zip",
    })
}

fn paths(plan: &BundlePlan) -> Vec<String> {
    plan.request["entries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| entry["archive_path"].as_str().unwrap().to_owned())
        .collect()
}

/// The closed recipe Brain's JSON editor and CLI send, with Brain's exact
/// admission rules: every member required, unknown members refused.
#[test]
fn the_composition_is_closed_and_bounded() {
    let ok = Composition::parse(&recipe(
        json!({"group_depth": null, "transformer_folders": true, "combine_per_group": false}),
    ))
    .unwrap();
    assert_eq!(ok.archive_layout, Layout::default());
    assert_eq!(ok.transformers, ["tx_a"]);
    let deep = Composition::parse(&recipe(
        json!({"group_depth": 2, "transformer_folders": false, "combine_per_group": true}),
    ))
    .unwrap();
    assert_eq!(deep.archive_layout.group_depth, Some(2));
    for bad in [
        json!([]),
        json!({"schema": COMPOSITION_SCHEMA}),
        json!({"schema": "other/v1", "transformers": [], "archive_layout": {"transformer_folders": true, "combine_per_group": false}, "force": false}),
        recipe(json!({"group_depth": 17, "transformer_folders": true, "combine_per_group": false})),
        recipe(json!({"group_depth": -1, "transformer_folders": true, "combine_per_group": false})),
        recipe(json!({"transformer_folders": true})),
        recipe(
            json!({"group_depth": null, "transformer_folders": true, "combine_per_group": false, "file_level": "root"}),
        ),
        json!({"schema": COMPOSITION_SCHEMA, "transformers": ["combined_transformer"], "archive_layout": {"transformer_folders": true, "combine_per_group": false}, "force": false}),
        json!({"schema": COMPOSITION_SCHEMA, "transformers": ["tx_a", "TX-A"], "archive_layout": {"transformer_folders": true, "combine_per_group": false}, "force": false}),
        json!({"schema": COMPOSITION_SCHEMA, "transformers": [" tx_a"], "archive_layout": {"transformer_folders": true, "combine_per_group": false}, "force": false}),
        json!({"schema": COMPOSITION_SCHEMA, "transformers": [], "archive_layout": {"transformer_folders": true, "combine_per_group": false}, "force": false, "project": "x"}),
    ] {
        let refusal = Composition::parse(&bad).unwrap_err();
        assert_eq!(refusal.code, COMPOSITION_INVALID, "{bad}");
    }
    let many: Vec<String> = (0..=MAX_TRANSFORMERS).map(|i| format!("tx_{i}")).collect();
    assert!(Composition::parse(&json!({"schema": COMPOSITION_SCHEMA, "transformers": many,
        "archive_layout": {"transformer_folders": true, "combine_per_group": false}, "force": false}))
    .is_err());
}

/// The legacy `--file-level` spellings map onto exactly the trees Brain's
/// `compoundedLegacyShape` produced.
#[test]
fn legacy_file_levels_are_the_generic_tree() {
    let shape = |level| {
        let layout = Layout::from_file_level(level, false).unwrap();
        (layout.group_depth, layout.transformer_folders)
    };
    assert_eq!(shape("transformer"), (None, true));
    assert_eq!(shape("sector"), (None, false));
    assert_eq!(shape("district"), (Some(1), false));
    assert_eq!(shape("root"), (Some(0), false));
    assert_eq!(
        Layout::from_file_level("flat", false).unwrap_err().code,
        COMPOSITION_INVALID
    );
    assert!(!Layout::from_file_level("root", false)
        .unwrap()
        .needs_grouping());
    assert!(Layout::from_file_level("root", true)
        .unwrap()
        .needs_grouping());
}

/// Folder names stay readable but always satisfy the Reporter's portable
/// member grammar (no separators, reserved names or trailing dots).
#[test]
fn folders_are_readable_and_portable() {
    assert_eq!(sanitize_folder("  Huye  "), "Huye");
    assert_eq!(sanitize_folder(""), "_unassigned");
    assert_eq!(sanitize_folder(".."), "_unassigned");
    assert_eq!(sanitize_folder("a/b\\c:d"), "a-b-c-d");
    assert_eq!(sanitize_folder("what?*<>|\""), "what------");
    assert_eq!(sanitize_folder("Phase 1."), "Phase 1");
    assert_eq!(sanitize_folder("con"), "_con");
    assert_eq!(sanitize_folder("COM1.txt"), "_COM1.txt");
    assert_eq!(sanitize_folder("COM10"), "COM10");
    let segments = vec!["Huye".to_owned(), "Tumba".to_owned()];
    assert_eq!(
        Layout::default().transformer_dir("tx_a", &segments),
        "Huye/Tumba/tx_a/"
    );
    let district = Layout::from_file_level("district", false).unwrap();
    assert_eq!(district.transformer_dir("tx_a", &segments), "Huye/");
    let root = Layout::from_file_level("root", false).unwrap();
    assert_eq!(root.transformer_dir("tx_a", &segments), "");
    assert_eq!(Layout::default().transformer_dir("tx_a", &[]), "tx_a/");
}

fn applied_plan() -> Value {
    json!({
        "purpose": "report_archive", "definition_ids": ["loc_admin_level_2", "loc_admin_level_3"],
        "plan_digest": "d".repeat(64), "revision": 4, "lifecycle": "active",
        "groups": [
            {"group_key": "k1", "values": [
                {"definition_id": "loc_admin_level_2", "values": ["huye"], "labels": ["Huye"]},
                {"definition_id": "loc_admin_level_3", "values": ["tumba"], "labels": []}],
             "transformers": ["tx_a"]},
            {"group_key": "k2", "unassigned": true, "values": [], "transformers": ["tx_b"]}
        ],
        "future_member": true
    })
}

/// The applied grouping gives each transformer its ordered segments (labels
/// first, values when unlabelled; the unassigned group files at
/// `_unassigned`); an archived or absent plan is flat.
#[test]
fn the_applied_grouping_names_each_transformers_folders() {
    let groups = group_paths(Some(&applied_plan())).unwrap();
    assert_eq!(groups.by_transformer["tx_a"], ["Huye", "tumba"]);
    assert_eq!(groups.by_transformer["tx_b"], ["", ""]);
    assert_eq!(groups.identity.as_ref().unwrap()["revision"], 4);
    assert_eq!(group_paths(None).unwrap(), GroupPaths::default());
    let mut archived = applied_plan();
    archived["lifecycle"] = json!("archived");
    assert_eq!(group_paths(Some(&archived)).unwrap(), GroupPaths::default());
    let mut doubled = applied_plan();
    doubled["groups"][1]["transformers"] = json!(["tx_a"]);
    assert_eq!(
        group_paths(Some(&doubled)).unwrap_err().code,
        GROUPING_INVALID
    );
    assert_eq!(
        group_paths(Some(&json!({"groups": "x"}))).unwrap_err().code,
        GROUPING_INVALID
    );
}

/// One archive: individual sets at the layout's folders, the overall combined
/// set at the root, per-group sets under their first-level folder, and a
/// deterministic manifest whose digest is the publication's input base.
#[test]
fn a_plan_files_every_member_and_pins_every_byte() {
    let composition = Composition {
        transformers: vec![],
        archive_layout: Layout {
            group_depth: Some(1),
            transformer_folders: true,
            combine_per_group: true,
        },
        force: false,
    };
    let groups = group_paths(Some(&applied_plan())).unwrap();
    let combined = vec![
        root_set(&["tx_a", "tx_b"]),
        CombinedSet {
            group: Some("Huye".into()),
            transformers: vec!["tx_a".into()],
            files: vec![file("combined_transformer.xlsx", 'd')],
        },
    ];
    let planned = plan_of(&composition, &groups, &combined).unwrap();
    assert_eq!(
        paths(&planned),
        [
            "Huye/tx_a/tx_a.xlsx",
            "Huye/tx_a/tx_a.kmz",
            "_unassigned/tx_b/tx_b.xlsx",
            "_unassigned/tx_b/tx_b.kmz",
            "combined_transformer.xlsx",
            "Huye/Huye_combined_transformer.xlsx",
        ]
    );
    assert_eq!(planned.entry_count, 7);
    assert_eq!(planned.request["output"], "/stage/compounded_reports.zip");
    assert_eq!(planned.request["pdf_collections"], json!([]));
    assert_eq!(planned.manifest["schema"], MANIFEST_SCHEMA);
    assert_eq!(
        planned.manifest["composition"]["transformers"],
        json!(["tx_a", "tx_b"])
    );
    assert_eq!(planned.manifest["transformers"][1]["revision"], 5);
    assert_eq!(planned.manifest["grouping"]["applied"], true);
    assert!(planned.prefix.starts_with("compounded-"));
    // Deterministic: the same inputs give the same input base.
    let again = plan_of(&composition, &groups, &combined).unwrap();
    assert_eq!(again.input_fingerprint, planned.input_fingerprint);
    // A changed byte is a changed archive.
    let mut changed = combined.clone();
    changed[0].files[0].sha256 = "e".repeat(64);
    assert_ne!(
        plan_of(&composition, &groups, &changed)
            .unwrap()
            .input_fingerprint,
        planned.input_fingerprint
    );
}

/// A smaller archive is never planned: a missing member, a missing root
/// combined set, a colliding member path, or an unpinned file refuses.
#[test]
fn a_plan_refuses_instead_of_shrinking() {
    let composition = Composition {
        transformers: vec![],
        archive_layout: Layout::from_file_level("root", false).unwrap(),
        force: false,
    };
    let groups = GroupPaths::default();
    assert_eq!(
        plan_of(&composition, &groups, &[]).unwrap_err().code,
        INPUTS_INVALID
    );
    let participating = vec!["tx_a".to_owned(), "tx_b".to_owned()];
    let members = vec![member("tx_a", 3)];
    let combined = vec![root_set(&["tx_a"])];
    let missing = plan(&PlanInput {
        project: "p1",
        composition: &composition,
        participating: &participating,
        members: &members,
        groups: &groups,
        combined: &combined,
        output: "/o.zip",
    });
    assert_eq!(missing.unwrap_err().code, INPUTS_INVALID);
    // Two transformers filed flat whose files share a name collide.
    let mut clash = vec![member("tx_a", 3), member("tx_b", 5)];
    clash[1].files[0].filename = "tx_a.xlsx".into();
    let collided = plan(&PlanInput {
        project: "p1",
        composition: &composition,
        participating: &participating,
        members: &clash,
        groups: &groups,
        combined: &combined,
        output: "/o.zip",
    });
    assert!(collided
        .unwrap_err()
        .message
        .contains("share the archive path"));
    let mut unpinned = vec![member("tx_a", 3), member("tx_b", 5)];
    unpinned[0].files[0].sha256 = "nothex".into();
    assert!(plan(&PlanInput {
        project: "p1",
        composition: &composition,
        participating: &participating,
        members: &unpinned,
        groups: &groups,
        combined: &combined,
        output: "/o.zip",
    })
    .is_err());
}

/// The Reporter's receipt must be this plan's archive, and the archive must
/// fit the publication door; an oversized one is refused by name.
#[test]
fn the_bundle_receipt_is_admitted_against_the_plan() {
    let composition = Composition {
        transformers: vec![],
        archive_layout: Layout::default(),
        force: false,
    };
    let planned = plan_of(
        &composition,
        &GroupPaths::default(),
        &[root_set(&["tx_a", "tx_b"])],
    )
    .unwrap();
    let receipt = |bytes: u64, entries: usize| {
        json!({"task": "export_compounded_report", "status": "completed",
            "output": "/stage/compounded_reports.zip", "bytes": bytes,
            "sha256": "f".repeat(64), "entries": entries, "additive": "accepted"})
    };
    let admitted = admit_receipt(&planned, &receipt(1234, planned.entry_count)).unwrap();
    assert_eq!(admitted.bytes, 1234);
    assert_eq!(
        admit_receipt(&planned, &receipt(1234, planned.entry_count + 1))
            .unwrap_err()
            .code,
        INPUTS_INVALID
    );
    assert_eq!(
        admit_receipt(
            &planned,
            &receipt(MAX_PUBLISHED_BYTES + 1, planned.entry_count)
        )
        .unwrap_err()
        .code,
        ARCHIVE_TOO_LARGE
    );
    let mut other_task = receipt(1, planned.entry_count);
    other_task["task"] = json!("export_transformer_report");
    assert!(admit_receipt(&planned, &other_task).is_err());
}
