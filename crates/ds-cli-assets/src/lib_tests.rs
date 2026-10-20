use super::*;

#[test]
fn every_command_of_the_domain_is_a_headless_project_command_or_a_native_one() {
    // No command of this domain needs a window: the catalogue commands
    // are headless project commands, the shared-reference commands were
    // already native, and the backup plan is local.
    for command in DOMAIN.commands {
        assert_ne!(
            command.requires,
            ds_cli_contract::spec::Requires::Window,
            "`{}` still claims a window",
            command.id
        );
        assert!(
            !command
                .args
                .iter()
                .any(|arg| arg.name == "desktop-descriptor"),
            "`{}` still declares the retired window path",
            command.id
        );
    }
}

#[test]
fn the_refusal_table_holds_the_headless_and_catalogue_sets_first() {
    let table = refusals::<23>(&[INVALID_QUERY]);
    assert_eq!(table[0].code, HEADLESS_REFUSALS[0].code);
    assert_eq!(table[15].code, "asset_not_found");
    assert_eq!(table[21].code, "assets_service_failed");
    assert_eq!(table[22].code, "invalid_query");
    let codes: std::collections::BTreeSet<&str> = table.iter().map(|r| r.code).collect();
    assert_eq!(codes.len(), table.len(), "a code is declared twice");
}

#[test]
fn the_kernels_sentence_becomes_the_code_the_commands_document() {
    assert_eq!(
        kernel_refused(
            "preview input is 40000000 bytes; the preview input bound is 33554432".into()
        )
        .code(),
        "asset_too_large"
    );
    assert_eq!(
        kernel_refused("member gis/poles.shp is not in the container".into()).code(),
        "invalid_member"
    );
    assert_eq!(
        kernel_refused("assets request must use ds.assets.request/v1".into()).code(),
        "asset_request_invalid"
    );
    assert_eq!(
        projected_unavailable("sys:transformers:T1").code(),
        "origin_read_unavailable"
    );
}

#[test]
fn every_default_sits_inside_the_bound_its_summary_states() {
    // `ds work` once shipped a --limit default above its bound, so every
    // call refused. A default is a value the parser will send unasked; it
    // must be one the validator accepts.
    let default = |arg: &Arg| {
        arg.default
            .expect("default")
            .parse::<i64>()
            .expect("number")
    };
    assert_eq!(default(&LIMIT_ARG), DEFAULT_PAGE_SIZE);
    assert!((1..=MAX_TREE_DEPTH).contains(&default(&DEPTH_ARG)));
    assert!((1..=MAX_PREVIEW_PAGES).contains(&default(&PAGES_ARG)));
    assert!((1..=MAX_PREVIEW_ROWS).contains(&default(&ROWS_ARG)));
}

#[test]
fn every_bound_a_summary_or_refusal_states_is_the_constant_it_names() {
    // A number printed in help is a hand copy of a constant, and the
    // parity suite only holds the constants. This holds the prose.
    assert!(LIMIT_ARG.summary.contains("1-200") && MAX_PAGE_SIZE == 200);
    assert!(DEPTH_ARG.summary.contains("1-8") && MAX_TREE_DEPTH == 8);
    assert!(PAGES_ARG.summary.contains("1-5") && MAX_PREVIEW_PAGES == 5);
    assert!(ROWS_ARG.summary.contains("1-200") && MAX_PREVIEW_ROWS == 200);
    assert!(INVALID_QUERY.when.contains("200") && MAX_QUERY_CHARS == 200);
    assert!(INVALID_LAYER_NAME.when.contains("80") && MAX_LAYER_NAME_CHARS == 80);
    assert!(INVALID_FOLDER_PATH.remedy.contains("contracts/2026/epc"));
}

#[test]
fn an_asset_id_is_minted_or_projected_and_nothing_else() {
    for good in [
        "a_7kq3nr2v0b1c",
        "sys:design_attachment:att_1:rev_2",
        "sys:pm_attachment:9",
        "sys:grid_revision:m1:r1",
        " a_000000000000 ",
    ] {
        assert!(asset_id(good, "asset").is_ok(), "`{good}` was refused");
    }
    for bad in [
        "",
        "a_",
        "a_7Kq3nR2v",
        "a_7kq3nr2v0b1",
        "a_7kq3nr2v0b1cd",
        "a-7kq3nr2v0b1c",
        "sys:",
        "sys:design_attachment",
        "sys:design_attachment:",
        "sys:Design:att",
        "sys::att",
        "t_4812",
    ] {
        assert_eq!(
            asset_id(bad, "asset").expect_err("must refuse").code(),
            "invalid_asset_id",
            "`{bad}` was accepted as an asset id"
        );
    }
    assert!(is_projected("sys:report_file:AGASHARU:abc"));
    assert!(!is_projected("a_7kq3nr2v0b1c"));
}

#[test]
fn a_folder_path_is_named_segments_without_traversal() {
    assert_eq!(
        folder_path(" contracts/2026/epc ", "folder").expect("valid"),
        "contracts/2026/epc"
    );
    assert!(folder_path("Transformers/AGASHARU/reports", "folder").is_ok());
    assert!(folder_path("MV models/Feeder 3", "folder").is_ok());
    for bad in [
        "",
        "/contracts",
        "contracts/",
        "contracts//epc",
        "contracts/./epc",
        "../contracts",
        "contracts\\epc",
        "contracts/ epc",
        "a/b/c/d/e/f/g/h/i/j/k/l/m/n/o/p/q",
    ] {
        assert_eq!(
            folder_path(bad, "folder").expect_err("must refuse").code(),
            "invalid_folder_path",
            "`{bad}` was accepted as a folder path"
        );
    }
}

#[test]
fn a_link_names_a_task_or_a_ds_object() {
    assert!(link("pm_task:t_4812", "link").is_ok());
    assert!(link("ds_object:transformer:TX-104", "link").is_ok());
    for bad in [
        "",
        "pm_task",
        "pm_task:",
        "pm_task:t_4812:extra",
        "ds_object:transformer",
        "ds_object::TX-104",
        "ds_object:transformer:",
        "ds_object:transformer:TX-104:extra",
        "PM_TASK:t_4812",
        "task:t_4812",
    ] {
        assert_eq!(
            link(bad, "link").expect_err("must refuse").code(),
            "invalid_link",
            "`{bad}` was accepted as a link"
        );
    }
}

#[test]
fn a_query_is_short_and_not_empty() {
    assert_eq!(query(" poles ", "query").expect("valid"), "poles");
    assert!(query(&"x".repeat(MAX_QUERY_CHARS), "query").is_ok());
    for bad in ["", "   ", &"x".repeat(MAX_QUERY_CHARS + 1)] {
        assert_eq!(
            query(bad, "query").expect_err("must refuse").code(),
            "invalid_query"
        );
    }
}

#[test]
fn an_index_time_reads_as_how_long_ago_it_was() {
    assert_eq!(epoch_seconds("1970-01-01T00:00:00Z"), Some(0));
    assert_eq!(epoch_seconds("2026-09-25T09:00:00Z"), Some(1_790_326_800));
    assert_eq!(
        epoch_seconds("2026-09-25T11:00:00.750+02:00"),
        epoch_seconds("2026-09-25T09:00:00Z")
    );
    assert_eq!(epoch_seconds("2026-09-25T09:00Z"), Some(1_790_326_800));
    for bad in [
        "",
        "2026-09-25",
        "2026-13-01T00:00:00Z",
        "yesterday",
        "2026-09-25T09:00:00",
    ] {
        assert_eq!(epoch_seconds(bad), None, "`{bad}` read as a time");
    }
    let now = "2026-09-25T10:00:00Z";
    for (then, expected) in [
        ("2026-09-25T09:59:30Z", "just now"),
        ("2026-09-25T09:48:00Z", "12m ago"),
        ("2026-09-25T07:00:00Z", "3h ago"),
        ("2026-09-20T10:00:00Z", "5d ago"),
        ("2026-05-25T10:00:00Z", "4mo ago"),
        ("2024-09-25T10:00:00Z", "2y ago"),
        // A clock a little ahead of the index's is not the future.
        ("2026-09-25T10:00:09Z", "just now"),
    ] {
        assert_eq!(ago(then, now).as_deref(), Some(expected), "{then}");
    }
    assert_eq!(ago("not a time", now), None);
}

#[test]
fn a_versions_chip_names_the_current_version_and_the_count() {
    assert_eq!(
        versions_chip(&json!({"count": 12, "current": "v3"})).as_deref(),
        Some("v3 · 12 versions")
    );
    assert_eq!(
        versions_chip(&json!({"count": 1, "current": 1})).as_deref(),
        Some("v1 · 1 version")
    );
    assert_eq!(
        versions_chip(&json!({"count": 2, "current": "7"})).as_deref(),
        Some("v7 · 2 versions")
    );
    assert_eq!(
        versions_chip(&json!({"count": 4})).as_deref(),
        Some("4 versions")
    );
    assert_eq!(versions_chip(&json!({"count": 0})), None);
    assert_eq!(versions_chip(&Value::Null), None);
}

#[test]
fn a_since_flag_is_a_calendar_date_or_a_timestamp() {
    for good in [
        "2026-09-01",
        "2028-02-29",
        "2026-09-01T00:00Z",
        "2026-09-01T08:12:00Z",
        "2026-09-01T08:12:00.250Z",
        "2026-09-01T08:12:00+02:00",
        "2026-09-01T08:12-05:00",
    ] {
        assert!(since(good, "since").is_ok(), "`{good}` was refused");
    }
    for bad in [
        "",
        "2026-9-1",
        "01-09-2026",
        "2026-13-01",
        "2026-02-29",
        "tomorrow",
        "2026-09-01T",
        "2026-09-01T08:12",
        "2026-09-01T25:00Z",
        "2026-09-01T08:60Z",
        "2026-09-01T08:12:00.Z",
        "2026-09-01T08:12:00+2:00",
        "2026-09-01Tü8:12Z",
    ] {
        assert_eq!(
            since(bad, "since").expect_err("must refuse").code(),
            "invalid_date",
            "`{bad}` was accepted as a --since value"
        );
    }
}
