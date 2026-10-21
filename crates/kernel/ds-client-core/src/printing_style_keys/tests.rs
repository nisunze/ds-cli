use super::*;
use crate::{AuthorizedTransport, TransportError};
use ds_command_kernel::printing::{context::style_key, default_layout};
use std::collections::{BTreeMap, VecDeque};

const PROJECT: &str = "czgmdwth_fixture";

fn revision(n: u8) -> String {
    format!("{n:x}").repeat(64)
}

/// A governed table that declares only `lv_lines`, exactly as ds-brain
/// answers `style.resolution.table`.
fn table(project: &str) -> Value {
    let document = json!({"type":"line","paint":{"line-width":0.6,"line-color":"#64748B"}});
    json!({
        "schema": ds_command_kernel::style_resolution::SCHEMA, "project_id": project,
        "revision_id": "b".repeat(64),
        "bindings": [{"key": style_key("lv_lines", "saved_design", "project"),
            "style_ref": "master/lv_lines_print", "revision_id": "a".repeat(64),
            "content_sha256": ds_command_kernel::style_plan::style_digest(&document),
            "scope": "global", "document": document}]
    })
}

fn layout(id: &str, refs: Value) -> Value {
    let mut layout = default_layout();
    layout.id = id.into();
    layout.style_refs = serde_json::from_value(refs).unwrap();
    serde_json::to_value(layout).unwrap()
}

/// One library per project: setup id → (revision, raw stored layout).
fn library() -> BTreeMap<String, (String, Value)> {
    BTreeMap::from([
        (
            "a0".to_string(),
            (
                revision(1),
                layout("a0", json!({"lv_lines":"master/lv_lines_print"})),
            ),
        ),
        (
            "a1".to_string(),
            (
                revision(2),
                layout("a1", json!({"village_boundaries":"master/villages_print"})),
            ),
        ),
        ("a2".to_string(), (revision(3), layout("a2", json!({})))),
        (
            "bad".to_string(),
            (revision(4), json!({"id":"bad","not_a_layout_field":true})),
        ),
    ])
}

fn ok(data: Value) -> TransportResponse {
    TransportResponse::new(
        200,
        serde_json::to_vec(&json!({"success":true,"data":data})).unwrap(),
    )
}

fn refused(status: u16, code: &str, message: &str) -> TransportResponse {
    TransportResponse::new(
        status,
        serde_json::to_vec(&json!({"success":false,"error":{"code":code,"message":message}}))
            .unwrap(),
    )
}

#[derive(Default)]
struct Brain {
    libraries: BTreeMap<String, BTreeMap<String, (String, Value)>>,
    /// Projects whose printing library refuses this identity.
    denied: Vec<String>,
    table: Option<(u16, &'static str, &'static str)>,
    saves: VecDeque<TransportResponse>,
    bodies: Vec<Value>,
    reads: usize,
}

macro_rules! unrelated {
    ($name:ident, $call:ident) => {
        fn $name(&mut self, _: crate::$call<'_>) -> Result<TransportResponse, TransportError> {
            panic!("the template migration made an unrelated call")
        }
    };
}

impl AuthorizedTransport for Brain {
    unrelated!(list_projects, ProjectListCall);
    unrelated!(transformer_context, TransformerContextCall);
    unrelated!(project_forms, ProjectFormsCall);
    unrelated!(project_form_editor, ProjectFormEditorCall);
    unrelated!(solar_snapshot, SolarSnapshotCall);
    unrelated!(survey_query, SurveyQueryCall);
    unrelated!(survey_entries_select, SurveyEntriesSelectCall);
    unrelated!(survey_entries_changes, SurveyEntriesChangesCall);
    unrelated!(survey_entry_create, SurveyEntryCreateCall);
    unrelated!(project_data, ProjectDataCall);
    unrelated!(upload_bytes, UploadBytesCall);
    unrelated!(survey_control, SurveyControlCall);
    unrelated!(layers, LayersCall);
    unrelated!(tiles, TileCall);
    unrelated!(project_report, ProjectReportCall);

    fn styles(&mut self, call: crate::StylesCall<'_>) -> Result<TransportResponse, TransportError> {
        let governance = call.governance.expect("only the governed table is read");
        assert!(matches!(
            governance.command,
            ds_command_kernel::style_governance::Command::Table
        ));
        self.reads += 1;
        Ok(match &self.table {
            Some((status, code, message)) => refused(*status, code, message),
            None => ok(table(&governance.project)),
        })
    }

    fn printing(
        &mut self,
        call: crate::PrintingCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        self.reads += 1;
        let project = call.ds_project.to_owned();
        if self.denied.contains(&project) {
            return Ok(refused(403, "FORBIDDEN", "no access"));
        }
        let library = self.libraries.get(&project).cloned().unwrap_or_default();
        Ok(match call.operation {
            crate::PrintingRequest::List {} => ok(json!({"setups": library
                .iter()
                .map(|(id, (revision, _))| json!({"id": id, "name": id, "revision": revision, "layout": null}))
                .collect::<Vec<_>>()})),
            crate::PrintingRequest::Get { id } => match library.get(id) {
                Some((revision, layout)) => ok(
                    json!({"id": id, "name": id.to_uppercase(), "revision": revision, "layout": layout}),
                ),
                None => refused(404, "print_setup_not_found", "Printing setup not found"),
            },
            _ => panic!("the template migration only reads the printing library"),
        })
    }

    fn print_style_keys(
        &mut self,
        call: PrintStyleKeysCall<'_>,
    ) -> Result<TransportResponse, TransportError> {
        assert_eq!(
            (call.method(), call.path()),
            ("POST", PRINT_STYLE_KEYS_PATH)
        );
        self.bodies
            .push(serde_json::from_slice(call.body().as_bytes()).unwrap());
        self.saves.pop_front().ok_or(TransportError::Unreachable)
    }
}

fn fixture() -> Brain {
    Brain {
        libraries: BTreeMap::from([(PROJECT.to_string(), library())]),
        ..Brain::default()
    }
}

fn run(
    brain: &mut Brain,
    project: &str,
    request: &PrintStyleKeysRequest,
) -> Result<Value, ClientError> {
    call(
        &crate::data_distribution::tests::profile_for_tests(),
        brain,
        Authority {
            bearer_token: "bearer",
            uid: "uid",
            email: "owner@example.com",
            device_id: None,
        },
        project,
        request,
    )
}

fn plan_request() -> PrintStyleKeysRequest {
    PrintStyleKeysRequest::Plan {
        id: None,
        choices: BTreeMap::new(),
    }
}

fn apply_request(expected: &str) -> PrintStyleKeysRequest {
    PrintStyleKeysRequest::Apply {
        id: None,
        choices: BTreeMap::new(),
        expected_plan_sha256: expected.into(),
    }
}

fn code(error: &ClientError) -> Option<&str> {
    error.service_refusal().and_then(ServiceRefusal::code)
}

#[test]
fn a_plan_is_decided_here_from_governed_reads_and_never_asks_brain_to_compute() {
    let mut brain = fixture();
    let plan = run(&mut brain, PROJECT, &plan_request()).unwrap();
    assert!(
        brain.bodies.is_empty(),
        "a plan never reaches the save route"
    );
    assert_eq!(plan["schema"], PRINT_STYLE_KEYS_PLAN_SCHEMA);
    assert_eq!(plan["scope"], "project");
    assert_eq!(plan["project_id"], PROJECT);
    assert_eq!(plan["catalogue_revision"], "b".repeat(64));
    assert_eq!(
        plan["counts"],
        json!({"blocked":1,"current":1,"migrate":1,"refused":1})
    );
    let status = |id: &str| {
        plan["setups"]
            .as_array()
            .unwrap()
            .iter()
            .find(|setup| setup["id"] == id)
            .unwrap()
            .clone()
    };
    assert_eq!(status("a0")["status"], "migrate");
    assert_eq!(status("a0")["revision"], revision(1));
    assert_eq!(status("a0")["name"], "A0");
    assert_eq!(
        status("a0")["plan"]["style_keys"]["lv_lines"]["role"],
        "project"
    );
    assert_eq!(status("a1")["status"], "blocked");
    assert_eq!(
        status("a1")["plan"]["findings"][0]["code"],
        "style_key_undeclared"
    );
    assert_eq!(status("a2")["status"], "current");
    assert_eq!(status("bad")["status"], "refused");
    assert_eq!(status("bad")["refusal"]["code"], "print_style_keys_invalid");
    assert!(
        status("a0").get("layout").is_none(),
        "the migrated layout is never shown"
    );
    let digest = plan["plan_sha256"].as_str().unwrap();
    assert!(super::digest(digest));
    // The same reads decide the same plan.
    let again = run(&mut brain, PROJECT, &plan_request()).unwrap();
    assert_eq!(again["plan_sha256"], digest);

    // One setup, and choices that name a setup the plan does not cover.
    let one = run(
        &mut brain,
        PROJECT,
        &PrintStyleKeysRequest::Plan {
            id: Some("a1".into()),
            choices: BTreeMap::new(),
        },
    )
    .unwrap();
    assert_eq!(one["id"], "a1");
    assert_eq!(one["setups"].as_array().unwrap().len(), 1);
    assert_ne!(one["plan_sha256"], digest);
    let key = style_key("village_boundaries", "cold_existing", "project");
    let stray = PrintStyleKeysRequest::Plan {
        id: Some("a1".into()),
        choices: BTreeMap::from([(
            "a0".to_string(),
            BTreeMap::from([("village_boundaries".to_string(), key)]),
        )]),
    };
    let error = run(&mut brain, PROJECT, &stray).unwrap_err();
    assert_eq!(code(&error), Some("print_style_keys_choice_unplanned"));
    let missing = PrintStyleKeysRequest::Plan {
        id: Some("absent".into()),
        choices: BTreeMap::new(),
    };
    assert_eq!(
        run(&mut brain, PROJECT, &missing).unwrap_err().kind(),
        ErrorKind::ResourceNotFound
    );
}

#[test]
fn apply_saves_only_a_still_reviewed_plan_under_each_setup_revision() {
    let mut brain = fixture();
    let plan = run(&mut brain, PROJECT, &plan_request()).unwrap();
    let digest = plan["plan_sha256"].as_str().unwrap().to_owned();

    // A stale review never reaches ds-brain.
    let error = run(&mut brain, PROJECT, &apply_request(&"c".repeat(64))).unwrap_err();
    assert_eq!(code(&error), Some("print_style_keys_plan_changed"));
    assert!(brain.bodies.is_empty());

    // A setup that moved after the review changes the plan.
    let mut moved = fixture();
    moved
        .libraries
        .get_mut(PROJECT)
        .unwrap()
        .get_mut("a2")
        .unwrap()
        .0 = revision(9);
    let error = run(&mut moved, PROJECT, &apply_request(&digest)).unwrap_err();
    assert_eq!(code(&error), Some("print_style_keys_plan_changed"));
    assert!(moved.bodies.is_empty());

    brain.saves.push_back(ok(json!({
        "schema": PRINT_STYLE_KEYS_SAVE_SCHEMA, "plan_sha256": digest,
        "migrated": [{"id":"a0","from_revision":revision(1),"to_revision":revision(7),
            "retained": format!("eds_project/{PROJECT}/printing_layout_versions/a0-{}", revision(1))}]
    })));
    let receipt = run(&mut brain, PROJECT, &apply_request(&digest)).unwrap();
    let body = &brain.bodies[0];
    assert_eq!(body["action"], "save");
    assert_eq!(body["scope"], "project");
    assert_eq!(body["project_id"], PROJECT);
    assert_eq!(body["plan_sha256"], digest);
    assert_eq!(body["catalogue_revision"], "b".repeat(64));
    let writes = body["writes"].as_array().unwrap();
    assert_eq!(writes.len(), 1, "only the migrate setup is written");
    assert_eq!(writes[0]["id"], "a0");
    assert_eq!(writes[0]["from_revision"], revision(1));
    assert_eq!(writes[0]["layout"]["style_refs"], json!({}));
    assert_eq!(
        writes[0]["layout"]["style_keys"]["lv_lines"]["entity_class"],
        "lv_lines"
    );
    assert!(body.get("choices").is_none() && body.get("expected_plan_sha256").is_none());
    assert_eq!(receipt["schema"], PRINT_STYLE_KEYS_RECEIPT_SCHEMA);
    assert_eq!(receipt["plan_sha256"], digest);
    assert_eq!(receipt["migrated"][0]["to_revision"], revision(7));
    assert_eq!(receipt["unchanged"], json!(["a2"]));
    assert_eq!(receipt["not_migrated"], json!(["a1", "bad"]));

    // ds-brain's own fence: a setup that moved between re-plan and write.
    brain.saves.push_back(refused(
        409,
        "print_style_keys_setup_changed",
        "a setup moved between the re-plan and its write; nothing was written",
    ));
    let error = run(&mut brain, PROJECT, &apply_request(&digest)).unwrap_err();
    assert_eq!(code(&error), Some("print_style_keys_setup_changed"));

    // A receipt for another plan is not this apply's receipt.
    brain.saves.push_back(ok(json!({
        "schema": PRINT_STYLE_KEYS_SAVE_SCHEMA, "plan_sha256": "d".repeat(64), "migrated": []
    })));
    assert_eq!(
        run(&mut brain, PROJECT, &apply_request(&digest))
            .unwrap_err()
            .kind(),
        ErrorKind::UnreadableResponse
    );

    // A ds-brain released before this change answers a typed route gap.
    for sentence in PREDATES_CORE_PLANS {
        brain
            .saves
            .push_back(refused(400, "VALIDATION_FAILED", sentence));
        assert_eq!(
            run(&mut brain, PROJECT, &apply_request(&digest))
                .unwrap_err()
                .kind(),
            ErrorKind::RouteUnavailable
        );
    }
}

#[test]
fn an_apply_with_nothing_to_migrate_writes_nothing() {
    let mut brain = Brain {
        libraries: BTreeMap::from([(
            PROJECT.to_string(),
            BTreeMap::from([("a2".to_string(), (revision(3), layout("a2", json!({}))))]),
        )]),
        ..Brain::default()
    };
    let plan = run(&mut brain, PROJECT, &plan_request()).unwrap();
    let receipt = run(
        &mut brain,
        PROJECT,
        &apply_request(plan["plan_sha256"].as_str().unwrap()),
    )
    .unwrap();
    assert!(brain.bodies.is_empty());
    assert_eq!(receipt["migrated"], json!([]));
    assert_eq!(receipt["unchanged"], json!(["a2"]));
}

#[test]
fn the_global_library_is_retired_and_refused_before_any_read() {
    let mut brain = fixture();
    for request in [plan_request(), apply_request(&"a".repeat(64))] {
        let error = run(&mut brain, "", &request).unwrap_err();
        assert_eq!(error.kind(), ErrorKind::InvalidInput);
        assert!(error
            .to_string()
            .contains("global printing library is retired"));
    }
    let census = PrintStyleKeysRequest::Census {
        cursor: None,
        limit: Some(5),
    };
    assert!(census.validate_for(PROJECT).is_err());
    assert!(PrintStyleKeysRequest::Census {
        cursor: None,
        limit: Some(26)
    }
    .validate_for("")
    .is_err());
    assert_eq!(brain.reads, 0);
    assert!(brain.bodies.is_empty());
}

#[test]
fn an_unseeded_catalogue_is_named_as_the_migration_documents_it() {
    let mut brain = fixture();
    brain.table = Some((
        412,
        "style_resolution_unseeded",
        "style_resolution_unseeded: manifest standard is not persisted",
    ));
    let error = run(&mut brain, PROJECT, &plan_request()).unwrap_err();
    assert_eq!(code(&error), Some("print_style_catalogue_unseeded"));
}

#[test]
fn a_census_lists_libraries_from_brain_and_plans_each_one_here() {
    let mut brain = fixture();
    brain
        .libraries
        .insert("second_fixture".into(), BTreeMap::new());
    brain.denied.push("private_fixture".into());
    brain.saves.push_back(ok(json!({
        "schema": PRINT_STYLE_KEYS_LIBRARIES_SCHEMA, "total_libraries": 4,
        "libraries": [PROJECT, "private_fixture", "second_fixture"], "next_cursor": "eyJhZnRlciI6InNlY29uZF9maXh0dXJlIn0"
    })));
    let census = run(
        &mut brain,
        "",
        &PrintStyleKeysRequest::Census {
            cursor: Some("eyJhZnRlciI6IiJ9".into()),
            limit: Some(3),
        },
    )
    .unwrap();
    assert_eq!(
        brain.bodies[0],
        json!({"action":"libraries","cursor":"eyJhZnRlciI6IiJ9","limit":3})
    );
    assert_eq!(census["schema"], PRINT_STYLE_KEYS_CENSUS_SCHEMA);
    assert_eq!(census["total_libraries"], 4);
    assert_eq!(census["next_cursor"], "eyJhZnRlciI6InNlY29uZF9maXh0dXJlIn0");
    let libraries = census["libraries"].as_array().unwrap();
    assert_eq!(libraries[0]["project_id"], PROJECT);
    assert_eq!(libraries[0]["counts"]["migrate"], 1);
    let planned = run(&mut brain, PROJECT, &plan_request()).unwrap();
    assert_eq!(libraries[0]["plan_sha256"], planned["plan_sha256"]);
    let a1 = libraries[0]["setups"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["id"] == "a1")
        .unwrap();
    assert_eq!(a1["findings"][0]["layer"], "village_boundaries");
    assert_eq!(libraries[1]["error"]["code"], "library_unreadable");
    assert_eq!(libraries[2]["counts"]["current"], 0);

    // A page that is not a library page is refused whole.
    brain.saves.push_back(ok(json!({
        "schema": PRINT_STYLE_KEYS_CENSUS_SCHEMA, "libraries": []
    })));
    assert_eq!(
        run(
            &mut brain,
            "",
            &PrintStyleKeysRequest::Census {
                cursor: None,
                limit: None
            }
        )
        .unwrap_err()
        .kind(),
        ErrorKind::UnreadableResponse
    );
}
