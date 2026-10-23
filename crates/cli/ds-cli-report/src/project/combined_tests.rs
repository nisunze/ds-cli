use super::*;

#[test]
fn json_composition_preflight_accepts_one_recipe_and_refuses_competing_scope_before_publication() {
    let recipe = r#"{"schema":"ds.compounded-report/v1","transformers":["tx_a"],"archive_layout":{"group_depth":null,"transformer_folders":true,"combine_per_group":false},"force":false}"#;
    let base = ["--project", "project_a", "--composition", recipe];
    let parse = |tokens: Vec<String>| ds_cli_contract::args::parse(&COMMAND, &tokens).unwrap();
    let valid = parse(base.iter().map(|v| (*v).into()).collect());
    assert!(preflight(&valid).is_ok());
    assert_eq!(
        composition_request(&valid).unwrap().unwrap().transformers,
        ["tx_a"]
    );
    for extra in [
        vec!["--transformer", "tx_b"],
        vec!["--group-by", "district"],
        vec!["--file-level", "root"],
        vec!["--force"],
        vec!["--combine-per-group"],
    ] {
        let tokens = base
            .iter()
            .chain(extra.iter())
            .map(|v| (*v).into())
            .collect();
        let refusal = preflight(&parse(tokens)).unwrap_err();
        assert_eq!(refusal.code(), "compounded_composition_conflict");
        assert!(refusal.remedy_text().is_some());
    }
    for raw in ["[]", "{", r#"{"transformers":["combined_transformer"]}"#] {
        let inputs = parse(
            ["--project", "project_a", "--composition", raw]
                .iter()
                .map(|v| (*v).into())
                .collect(),
        );
        assert_eq!(
            preflight(&inputs).unwrap_err().code(),
            "compounded_composition_invalid"
        );
    }
}

/// Each archive is one line naming its group, so an operator can see
/// which city a prefix belongs to without opening JSON.
#[test]
fn a_grouped_run_renders_one_line_per_group_archive() {
    let data = json!({
        "project": {"ds_project": "p1"}, "lane": "canary", "published_count": 2,
        "grouping": {"transformer_count": 20},
        "groups": [
            {"path": [{"key": "city", "value": "bere"}], "transformer_count": 12,
             "status": "success", "prefix": "run-bere"},
            {"path": [{"key": "city", "value": "_unassigned"}], "transformer_count": 8,
             "status": "success", "prefix": "run-rest"},
        ],
    });
    let rendered = render(&data);
    assert!(
        rendered.contains("2 Compounded Report archive(s) of 2 group(s)"),
        "{rendered}"
    );
    assert!(rendered.contains("city=bere"), "{rendered}");
    assert!(rendered.contains("run-bere"), "{rendered}");
    assert!(rendered.contains("city=_unassigned"), "{rendered}");
}

/// Grouping and naming transformers are alternatives, and every grouping
/// refusal is one this command declares — as is everything the combined set
/// computed here, the bundle and the seal can refuse, each code once.
#[test]
fn the_grouping_flags_are_declared_once_with_their_refusals() {
    for flag in ["group-by", "where", "transformer", "core-state-dir"] {
        assert!(COMMAND.arg(flag).is_some(), "--{flag}");
    }
    let declared: Vec<&str> = COMMAND.refusals.iter().map(|r| r.code).collect();
    for code in [
        "combined_group_scope_conflict",
        "combined_groups_too_many",
        "combined_groups_empty",
        "combined_groups_partial",
        "combined_inputs_not_current",
        "compounded_grouping_invalid",
        "compounded_inputs_invalid",
        "compounded_archive_too_large",
        "compounded_store_unreadable",
        "reporter_engine_missing",
        "module_receipt_invalid",
        "combined_report_collision_limit_exceeded",
        "report_publish_scope_changed",
    ] {
        assert!(declared.contains(&code), "{code} undeclared");
    }
    let mut unique = declared.clone();
    unique.sort_unstable();
    unique.dedup();
    assert_eq!(
        unique.len(),
        declared.len(),
        "a refusal code is declared twice"
    );
}

fn cause(name: &str, code: &str) -> Value {
    json!({"transformer": name, "code": code, "detail": ""})
}

/// The archive has its own command.
#[test]
fn the_archive_command_is_named_compounded() {
    assert_eq!(COMMAND.id, "report.project.compounded");
    assert_eq!(COMMAND.path, &["report", "project", "compounded"]);
    assert!(
        COMMAND.summary.contains("Compounded Report ZIP"),
        "{}",
        COMMAND.summary
    );
}

/// A room with no report here, or an older one than its saved design, is a
/// refusal naming the rooms and the export that makes them current — before
/// any archive is composed.
#[test]
fn unready_inputs_refuse_and_name_the_export() {
    let causes = vec![
        cause("tx_a", "report_not_published"),
        cause("tx_b", "report_stale"),
    ];
    let failure = readiness_refusal(&causes)
        .expect("the predicate answers")
        .expect("unready rooms refuse");
    assert_eq!(failure.code(), "combined_inputs_not_current");
    assert!(failure.message().contains("tx_a"), "{}", failure.message());
    assert!(failure.message().contains("tx_b"), "{}", failure.message());
    assert!(
        failure.message().contains("No archive was composed"),
        "{}",
        failure.message()
    );
    assert!(
        failure
            .remedy_text()
            .unwrap_or_default()
            .contains("ds report project export --transformer tx_a --transformer tx_b"),
        "{:?}",
        failure.remedy_text()
    );
    let detail = failure.detail_value().expect("detail travels");
    assert_eq!(detail["causes"][1]["code"], "report_stale");
}

/// A complete run is not turned into a refusal by the gate.
#[test]
fn a_complete_run_is_not_refused() {
    assert!(
        readiness_refusal(&[])
            .expect("the predicate answers")
            .is_none()
    );
}

/// The receipt reports what was ASKED FOR, never the shared vocabulary's echo.
#[test]
fn the_requested_grouping_survives_the_shared_vocabulary() {
    for requested in [true, false] {
        let layout = archive_layout("transformer", requested);
        assert_eq!(
            layout["combine_per_group"],
            json!(requested),
            "the receipt must report the grouping the caller asked for"
        );
        assert_eq!(layout["file_level"], json!("transformer"));
        assert_eq!(layout["level_key"], "pctl_layout_transformer");
    }
    assert_eq!(
        archive_layout("district", true)["label_key"],
        "pctl_layout_per_district"
    );
}

/// The descriptor says where the archive is computed, how it is published,
/// and that the layout is a request, not a tree.
#[test]
fn the_descriptor_states_the_edge_composition_and_its_publication() {
    for phrase in [
        "the layout asked for, never the tree achieved",
        "`_unassigned/`",
        "ds report outbox drain",
        "ds report publication show",
    ] {
        assert!(
            COMMAND.output.contains(phrase),
            "{phrase}: {}",
            COMMAND.output
        );
    }
    let purpose = COMMAND.purpose.to_ascii_lowercase();
    for phrase in [
        "on this machine",
        "applied report_archive grouping",
        "export-compounded_report",
        "report project export",
    ] {
        assert!(purpose.contains(phrase), "{phrase}: {}", COMMAND.purpose);
    }
    assert!(!purpose.contains("cloud"), "{}", COMMAND.purpose);
    assert!(
        FILE_LEVEL_ARG.summary.starts_with("Requested folder level"),
        "{}",
        FILE_LEVEL_ARG.summary
    );
}

/// The refusal a reader sees first is the one line the render prints.
#[test]
fn the_render_says_compounded_report_and_how_to_publish() {
    let data = json!({
        "project": {"ds_project": "p1"},
        "lane": "stable", "status": "success", "prefix": "compounded-0123",
        "individual_artifact_transformer_count": 2,
        "missing_individual_artifact_count": 0,
        "archives": ["ds-reporter:v1:batch/zip"],
        "publication": {"next": "ds report outbox drain --project p1"},
    });
    let rendered = render(&data);
    assert!(rendered.contains("Compounded Report"), "{rendered}");
    assert!(
        rendered.contains("ds report outbox drain --project p1"),
        "{rendered}"
    );
}

/// The legacy level tokens are the parser's choices, mapped once.
#[test]
fn legacy_levels_are_the_declared_choices() {
    for level in ["transformer", "sector", "district", "root"] {
        assert_eq!(level_token(level), level);
        assert!(Layout::from_file_level(level_token(level), false).is_ok());
    }
}

/// The edge composition end to end, with the installed Reporter: the kernel
/// plans the archive, the Reporter's closed bundle task builds the ZIP by
/// request and response while every outbound connection is trapped, and the
/// ZIP is sealed into a publication queue as `export-compounded_report` with
/// the exact bytes the Reporter reported. Run explicitly:
/// `DS_REPORT_BIN=<reporter> cargo test -p ds-cli-report compounded_edge -- --ignored --test-threads=1`.
#[test]
#[ignore = "needs an installed Reporter in DS_REPORT_BIN; mutates process environment"]
fn compounded_edge_composition_bundles_offline_and_seals_for_the_door() {
    use ds_command_kernel::report::compounded::{GroupPaths, Member, PlanInput, StagedFile};
    use sha2::Digest;
    assert!(
        std::env::var_os("DS_REPORT_BIN").is_some(),
        "set DS_REPORT_BIN to an installed reporter"
    );
    // Any socket the composition opens lands here, and fails the test.
    let trap = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    trap.set_nonblocking(true).unwrap();
    let proxy = format!("http://{}", trap.local_addr().unwrap());
    // SAFETY: the test runs alone (`--test-threads=1`, ignored by default).
    unsafe {
        for key in [
            "HTTP_PROXY",
            "HTTPS_PROXY",
            "ALL_PROXY",
            "http_proxy",
            "https_proxy",
        ] {
            std::env::set_var(key, &proxy);
        }
        std::env::set_var("NO_PROXY", "");
    }
    let root = tempfile::tempdir().unwrap();
    let stage = root.path().join("stage");
    std::fs::create_dir_all(&stage).unwrap();
    let staged = |name: &str, body: &[u8]| {
        let path = stage.join(name);
        std::fs::write(&path, body).unwrap();
        StagedFile {
            filename: name.to_owned(),
            source: path.display().to_string(),
            sha256: format!("{:x}", sha2::Sha256::digest(body)),
            size_bytes: body.len() as u64,
        }
    };
    let members = vec![
        Member {
            transformer: "tx_a".into(),
            revision: 3,
            batch_id: "b_a".into(),
            files: vec![staged("tx_a.xlsx", b"individual a")],
        },
        Member {
            transformer: "tx_b".into(),
            revision: 7,
            batch_id: "b_b".into(),
            files: vec![staged("tx_b.xlsx", b"individual b")],
        },
    ];
    let combined = vec![decisions::CombinedSet {
        group: None,
        transformers: vec!["tx_a".into(), "tx_b".into()],
        files: vec![staged("combined_transformer.xlsx", b"combined set")],
    }];
    let composition = Composition {
        transformers: vec![],
        archive_layout: Layout::default(),
        force: false,
    };
    let participating = vec!["tx_a".to_owned(), "tx_b".to_owned()];
    let output = stage.join(decisions::ARCHIVE_FILENAME);
    let plan = decisions::plan(&PlanInput {
        project: "p1",
        composition: &composition,
        participating: &participating,
        members: &members,
        groups: &GroupPaths::default(),
        combined: &combined,
        output: &output.display().to_string(),
    })
    .unwrap();
    let (receipt, engine) = bundle("stable", &plan, &stage).expect("the Reporter bundles");
    let bytes = std::fs::read(&output).unwrap();
    assert_eq!(receipt.bytes, bytes.len() as u64);
    assert_eq!(
        receipt.sha256,
        format!("{:x}", sha2::Sha256::digest(&bytes))
    );
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(&bytes)).unwrap();
    let names: Vec<String> = archive.file_names().map(str::to_owned).collect();
    for expected in [
        "tx_a/tx_a.xlsx",
        "tx_b/tx_b.xlsx",
        "combined_transformer.xlsx",
        "manifest.json",
    ] {
        assert!(
            names.iter().any(|name| name == expected),
            "{expected} not in {names:?}"
        );
    }
    let mut manifest = String::new();
    std::io::Read::read_to_string(
        &mut archive.by_name("manifest.json").unwrap(),
        &mut manifest,
    )
    .unwrap();
    let manifest: Value = serde_json::from_str(&manifest).unwrap();
    assert_eq!(manifest, plan.manifest);
    assert_eq!(
        trap.accept().unwrap_err().kind(),
        std::io::ErrorKind::WouldBlock,
        "the composition opened a network connection"
    );

    let store =
        ds_sync_runtime::open_store(&root.path().join("queue").join("store.sqlite")).unwrap();
    let fence = ds_command_kernel::sync_store::Fence {
        account: "owner_uid".into(),
        deployment: "stable".into(),
        install_id: "install".into(),
    };
    let queue = PublicationQueue::at(
        root.path().join("queue").join("report-artifacts"),
        store,
        fence,
    );
    let (cached, publication) = seal_archive(
        &queue,
        "owner_uid",
        "p1",
        &engine,
        &plan,
        &receipt,
        &output,
        "run1",
        &|| Ok(()),
    )
    .expect("the archive seals");
    assert!(!cached);
    assert_eq!(publication["queued"], true);
    let batches =
        ds_report_artifacts::list_project_publication_batches_from_root(&queue.root, "p1").unwrap();
    assert_eq!(batches.len(), 1);
    assert_eq!(batches[0].operation, decisions::OPERATION);
    assert_eq!(batches[0].engine, "network_reporter");
    assert_eq!(batches[0].input_base_fingerprint, plan.input_fingerprint);
    assert_eq!(batches[0].outputs.len(), 1);
    assert_eq!(batches[0].outputs[0].filename, decisions::ARCHIVE_FILENAME);
    assert_eq!(batches[0].outputs[0].sha256, receipt.sha256);
    // The same archive again is the recorded publication, not a second one.
    let (again, _) = seal_archive(
        &queue,
        "owner_uid",
        "p1",
        &engine,
        &plan,
        &receipt,
        &output,
        "run2",
        &|| Ok(()),
    )
    .unwrap();
    assert!(again, "an identical archive must not be queued twice");
}
