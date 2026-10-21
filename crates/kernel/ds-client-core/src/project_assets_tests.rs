use super::*;

fn list() -> Command {
    Command::List {
        folder_id: None,
        kind: None,
        status: None,
        sensitivity: None,
        since: None,
        limit: 50,
        cursor: None,
    }
}

#[test]
fn retained_index_pages_share_online_project_count_cursor_and_meta_admission() {
    let list = index_list();
    let page = json!({"assets":[{"schema":"ds.assets.asset/v1","asset_id":"a_7kq3nr2v0b1c","project_id":"p"}],"more":false,"index":{"schema":INDEX_SCHEMA,"generation":7}});
    assert!(verify_index_page("p", &list, &page).is_ok());
    let mut unchanged = page.clone();
    unchanged["assets"] = json!([]);
    unchanged["not_modified"] = json!(true);
    assert!(verify_index_page("p", &list, &unchanged).is_err());
    let mut foreign = page.clone();
    foreign["assets"][0]["project_id"] = json!("other");
    assert!(verify_index_page("p", &list, &foreign).is_err());
    let mut excess = page.clone();
    excess["assets"] = json!(vec![page["assets"][0].clone(); usize::from(list.limit) + 1]);
    assert!(verify_index_page("p", &list, &excess).is_err());
    for (field, value) in [
        ("more", json!("false")),
        ("next_cursor", json!(123)),
        ("index", json!({"schema":"foreign","generation":7})),
    ] {
        let mut bad = page.clone();
        bad[field] = value;
        assert!(verify_index_page("p", &list, &bad).is_err());
    }
    let scope = ds_command_kernel::assets::index_read::Scope {
        uid: "u".into(),
        email: "owner@example.com".into(),
        lane: "canary".into(),
        audience: "credential:a".into(),
        project: "p".into(),
    };
    assert_eq!(
        index_read_identity(&scope, &list).unwrap()["query"],
        index_read_query("p", &list).unwrap()
    );
}

#[test]
fn conditional_index_receipt_is_exact_qualified_and_not_a_full_page() {
    let list = index_list();
    let basis = "a".repeat(64);
    let command = Command::ConditionalIndexList {
        list: list.clone(),
        known_basis: basis.clone(),
    };
    assert!(command.validate("p").is_ok());
    let mut expected = Command::IndexList(list.clone()).payload("p");
    expected["known_basis"] = json!(basis);
    assert_eq!(command.payload("p"), expected);
    let receipt = json!({"assets":[],"more":false,"not_modified":true,"basis":basis,"index":{"schema":INDEX_SCHEMA,"generation":7,"stale_sources":["grid_exports"],"truncated_sources":[]}});
    assert!(verify_conditional_index_response("p", &list, &basis, &receipt).is_ok());
    assert!(verify_index_page("p", &list, &receipt).is_err());
    for (field, value) in [
        ("basis", json!("b".repeat(64))),
        ("assets", json!([{}])),
        ("more", json!(true)),
        ("next_cursor", json!("cursor")),
    ] {
        let mut bad_receipt = receipt.clone();
        bad_receipt[field] = value;
        assert!(verify_conditional_index_response("p", &list, &basis, &bad_receipt).is_err());
    }
    for invalid in ["A".repeat(64), "a".repeat(63), "g".repeat(64)] {
        assert!(Command::ConditionalIndexList {
            list: list.clone(),
            known_basis: invalid
        }
        .validate("p")
        .is_err());
    }
    let mut refresh = list.clone();
    refresh.refresh = true;
    assert!(Command::ConditionalIndexList {
        list: refresh,
        known_basis: basis.clone()
    }
    .validate("p")
    .is_err());
    let legacy = json!({"assets":[],"more":false,"index":{"schema":INDEX_SCHEMA,"generation":7}});
    assert!(verify_conditional_index_response("p", &list, &basis, &legacy).is_ok());
}

#[test]
fn the_action_vocabulary_is_closed_and_the_payloads_are_the_handlers_own() {
    assert_eq!(Command::Folders.action(), "tree");
    assert_eq!(list().action(), "list");
    let attach = Command::Attach {
        asset_id: "a_7kq3nr2v0b1c".into(),
        link: json!({"kind": "pm_task", "id": "t_1"}),
        detach: false,
    };
    assert_eq!(attach.action(), "attach");
    assert_eq!(attach.payload("p")["link"]["id"], "t_1");
    let detach = Command::Attach {
        asset_id: "a_7kq3nr2v0b1c".into(),
        link: json!({"kind": "pm_record", "id": "R1"}),
        detach: true,
    };
    assert_eq!(detach.action(), "detach");
    let classify = Command::Classify {
        asset_id: "a_7kq3nr2v0b1c".into(),
        kind: None,
        status: Some("durable".into()),
        sensitivity: None,
        folder_id: Some("f_1".into()),
        owner: Some("lead@ds.rw".into()),
        reason: Some("settled".into()),
        expected_version: 3,
    };
    let payload = classify.payload("p");
    assert_eq!(payload["classify"]["expected_version"], 3);
    assert_eq!(payload["classify"]["owner"]["kind"], "user");
    assert!(payload["classify"].get("kind").is_none());
    let folder = Command::FolderCreate {
        path: "contracts/2026".into(),
        default_sensitivity: Some("confidential".into()),
        default_status: None,
    };
    assert_eq!(folder.payload("p")["folder"]["path"], "contracts/2026");
    let download = Command::Download {
        asset_id: "a_7kq3nr2v0b1c".into(),
    };
    assert_eq!(download.payload("p")["surface"], "cli");
}

#[test]
fn a_request_outside_the_catalogues_vocabulary_is_refused_before_it_is_sent() {
    assert!(list().validate("p").is_ok());
    let refused = |command: Command| command.validate("p").expect_err("refused").kind();
    let list_with = |limit: u16, kind: Option<&str>| Command::List {
        folder_id: None,
        kind: kind.map(str::to_owned),
        status: None,
        sensitivity: None,
        since: None,
        limit,
        cursor: None,
    };
    assert_eq!(refused(list_with(201, None)), ErrorKind::InvalidInput);
    assert_eq!(refused(list_with(50, Some("pdf"))), ErrorKind::InvalidInput);
    assert_eq!(
        refused(Command::Get {
            asset_id: "sys:transformers:T1".into()
        }),
        ErrorKind::InvalidInput
    );
    assert_eq!(
        refused(Command::FolderCreate {
            path: "/contracts".into(),
            default_sensitivity: None,
            default_status: None
        }),
        ErrorKind::InvalidInput
    );
    assert_eq!(
        refused(Command::Classify {
            asset_id: "a_7kq3nr2v0b1c".into(),
            kind: None,
            status: None,
            sensitivity: None,
            folder_id: None,
            owner: None,
            reason: Some("nothing changes".into()),
            expected_version: 1,
        }),
        ErrorKind::InvalidInput
    );
    assert_eq!(
        refused(Command::Attach {
            asset_id: "a_7kq3nr2v0b1c".into(),
            link: json!({"kind": "tag", "definition_id": "city", "value": "x"}),
            detach: false,
        }),
        ErrorKind::InvalidInput
    );
    assert_eq!(
        refused(Command::Ingest(IngestRequest {
            name: "a.pdf".into(),
            size: 0,
            sha256: "0".repeat(64),
            content_type: None,
            folder_id: None,
            sensitivity: None,
            kind: "doc".into(),
            format: "pdf".into(),
        })),
        ErrorKind::InvalidInput
    );
    assert!(Command::Ingest(IngestRequest {
        name: "a.pdf".into(),
        size: 10,
        sha256: "a".repeat(64),
        content_type: Some("application/pdf".into()),
        folder_id: Some("f_1".into()),
        sensitivity: Some("confidential".into()),
        kind: "doc".into(),
        format: "pdf".into(),
    })
    .validate("p")
    .is_ok());
}

fn index_list() -> IndexList {
    IndexList {
        order: IndexOrder::default(),
        folder: None,
        kind: None,
        status: None,
        sensitivity: None,
        since: None,
        limit: 50,
        cursor: None,
        refresh: false,
    }
}

#[test]
fn the_index_forms_carry_view_index_and_only_the_keys_they_set() {
    // The timeline is the default: a bare index page asks for `recent`.
    let bare = Command::IndexList(index_list());
    assert_eq!(bare.action(), "list");
    assert_eq!(
        bare.payload("p"),
        json!({"action":"list","ds_project":"p","view":"index","order":"recent","limit":50})
    );
    let full = Command::IndexList(IndexList {
        order: IndexOrder::Name,
        folder: Some("Transformers/TX-104".into()),
        kind: Some("doc".into()),
        status: Some("durable".into()),
        sensitivity: Some("internal".into()),
        since: Some("2026-09-01".into()),
        limit: 200,
        cursor: Some("g7.c".into()),
        refresh: true,
    });
    assert_eq!(
        full.payload("p"),
        json!({"action":"list","ds_project":"p","view":"index","order":"name",
               "folder":"Transformers/TX-104","kind":"doc","status":"durable",
               "sensitivity":"internal","since":"2026-09-01","limit":200,
               "cursor":"g7.c","refresh":true})
    );
    let tree = Command::IndexTree { refresh: false };
    assert_eq!(tree.action(), "tree");
    assert_eq!(
        tree.payload("p"),
        json!({"action":"tree","ds_project":"p","view":"index"})
    );
    assert_eq!(
        Command::IndexTree { refresh: true }.payload("p")["refresh"],
        true
    );
    let get = Command::IndexGet {
        asset_id: "sys:transformers:TX-104".into(),
    };
    assert_eq!(get.action(), "get");
    assert_eq!(
        get.payload("p"),
        json!({"action":"get","ds_project":"p","view":"index","asset_id":"sys:transformers:TX-104"})
    );
    assert!(get.is_index() && tree.is_index() && full.is_index());
    assert!(!list().is_index() && !Command::Folders.is_index());
    assert_eq!(IndexOrder::parse("name"), Some(IndexOrder::Name));
    assert_eq!(IndexOrder::parse("oldest"), None);
}

#[test]
fn an_index_request_outside_its_vocabulary_is_refused_before_it_is_sent() {
    let ok = |command: Command| command.validate("p").is_ok();
    assert!(ok(Command::IndexGet {
        asset_id: "a_7kq3nr2v0b1c".into()
    }));
    assert!(ok(Command::IndexGet {
        asset_id: "sys:design_attachment:att_1:rev_2".into()
    }));
    for bad in [
        "",
        "sys:",
        "sys:Transformers:T1",
        "sys::T1",
        "sys:x:",
        " sys:x:1",
        "t_1",
    ] {
        assert!(
            !ok(Command::IndexGet {
                asset_id: bad.into()
            }),
            "`{bad}` was accepted"
        );
    }
    let with = |edit: fn(&mut IndexList)| {
        let mut list = index_list();
        edit(&mut list);
        Command::IndexList(list)
    };
    assert!(ok(with(|l| l.folder = Some("MV models/Feeder 3".into()))));
    assert!(!ok(with(|l| l.folder = Some("/Transformers".into()))));
    assert!(!ok(with(|l| l.folder = Some("Transformers//TX".into()))));
    assert!(!ok(with(|l| l.limit = 0)));
    assert!(!ok(with(|l| l.limit = 201)));
    assert!(!ok(with(|l| l.kind = Some("pdf".into()))));
    assert!(ok(with(
        |l| l.cursor = Some("c".repeat(MAX_INDEX_CURSOR_CHARS))
    )));
    assert!(!ok(with(
        |l| l.cursor = Some("c".repeat(MAX_INDEX_CURSOR_CHARS + 1))
    )));
    assert!(Command::IndexTree { refresh: true }.validate("").is_err());
}

#[test]
fn the_edge_only_sources_are_the_kernels_own_names_and_roots() {
    use ds_command_kernel::assets::{tree, TreeRequest, REQUEST_SCHEMA, TREE_SOURCE_NAMES};
    // A tree of no sources marks every root with the sources that feed
    // it; each edge-only source must feed the root this table names.
    let empty = tree::build_value(&TreeRequest {
        schema: REQUEST_SCHEMA.to_owned(),
        sources: Default::default(),
        folders: Vec::new(),
        assets: Vec::new(),
        query: None,
        link: None,
        folder: None,
        depth: Some(1),
        kind: None,
    })
    .expect("an empty tree");
    for (source, root) in EDGE_ONLY_SOURCES {
        assert!(TREE_SOURCE_NAMES.contains(&source), "{source}");
        let folder = empty["folders"]
            .as_array()
            .expect("roots")
            .iter()
            .find(|folder| folder["path"] == root)
            .unwrap_or_else(|| panic!("no root {root}"));
        assert!(
            folder["not_loaded_sources"]
                .as_array()
                .is_some_and(|sources| sources.contains(&json!(source))),
            "{source} does not feed {root}"
        );
    }
}

#[test]
fn an_index_row_is_this_projects_or_a_projected_row_of_none() {
    let row = |id: &str, project: Value| json!({"schema":"ds.assets.asset/v1","asset_id":id,"project_id":project});
    assert!(own_index_row(&row("a_7kq3nr2v0b1c", json!("p")), "p").is_ok());
    assert!(own_index_row(&row("a_7kq3nr2v0b1c", json!("")), "p").is_err());
    assert!(own_index_row(&row("sys:transformers:T1", json!("")), "p").is_ok());
    assert!(own_index_row(&row("sys:transformers:T1", Value::Null), "p").is_ok());
    assert!(own_index_row(&row("sys:transformers:T1", json!("other")), "p").is_err());
    assert!(own_index_row(&row("T1", json!("p")), "p").is_err());
    let mut foreign_schema = row("a_7kq3nr2v0b1c", json!("p"));
    foreign_schema["schema"] = json!("ds.assets.asset/v2");
    assert!(own_index_row(&foreign_schema, "p").is_err());
}

#[test]
fn a_refusal_carries_the_servers_code_sentence_and_rule() {
    let body = br#"{"success":false,"error":{"code":"ASSET_REFUSED","message":"an ingest may not loosen its folder","details":{"reason":"implicit_loosening","folder_default":"confidential","requested":"internal"}}}"#;
    let error = refusal(400, body);
    assert_eq!(error.kind(), ErrorKind::InvalidInput);
    let served = error.service_refusal().expect("carried");
    assert_eq!(served.code(), Some("asset_refused"));
    assert_eq!(
        served.message(),
        Some("an ingest may not loosen its folder [implicit_loosening]")
    );
    let hidden = refusal(
        404,
        br#"{"success":false,"error":{"code":"NOT_FOUND","message":"Asset not found"}}"#,
    );
    assert_eq!(hidden.kind(), ErrorKind::ResourceNotFound);
    let stale = refusal(
        409,
        br#"{"success":false,"error":{"code":"CONFLICT","message":"expected_version 2, found 3"}}"#,
    );
    assert_eq!(
        stale.service_refusal().and_then(|r| r.code()),
        Some("conflict")
    );
    let forbidden = refusal(403, br#"{"success":false,"error":{"code":"ASSET_CLASS_FORBIDDEN","message":"restricted needs assets.read.commercial","details":{"sensitivity":"restricted","capability":"assets.read.commercial"}}}"#);
    assert_eq!(forbidden.kind(), ErrorKind::AuthenticationRejected);
    assert_eq!(refusal(501, b"{}").kind(), ErrorKind::RouteUnavailable);
    assert_eq!(refusal(503, b"upstream").kind(), ErrorKind::Transient);
}
