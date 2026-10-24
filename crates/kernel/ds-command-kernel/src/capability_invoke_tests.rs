//! The invoke door answers the CLI's own envelope for every executor kind:
//! an in-page Kernel owner, the Network unit, Core through the host's Core
//! door, a pending and a discarded row, an unknown id and a malformed
//! request. CLI refusals quoted here were printed by the installed
//! `ds --output json` (f650a1a6); the node harness compares them live.
use super::*;
use futures_util::FutureExt;
use std::cell::RefCell;

/// A host whose doors answer at once and record what they were asked.
#[derive(Default)]
struct Fake {
    core: Option<Result<String, CommandRefusal>>,
    unit: Option<Result<Value, CommandRefusal>>,
    owner: Option<Result<Value, CommandRefusal>>,
    asked: RefCell<Vec<Value>>,
}

impl Doors for Fake {
    async fn core(
        &self,
        id: &'static str,
        project: &str,
        args: Map<String, Value>,
    ) -> Result<String, CommandRefusal> {
        self.asked
            .borrow_mut()
            .push(json!({"door": "core", "id": id, "project": project, "args": args}));
        self.core.clone().expect("the core door was not expected")
    }
    async fn unit(&self, id: &'static str, input: Value) -> Result<Value, CommandRefusal> {
        self.asked
            .borrow_mut()
            .push(json!({"door": "unit", "id": id, "input": input}));
        self.unit.clone().expect("the unit door was not expected")
    }
    async fn owner(
        &self,
        owner: &'static PageOwner,
        id: &'static str,
        request: Map<String, Value>,
    ) -> Result<Value, CommandRefusal> {
        self.asked
            .borrow_mut()
            .push(json!({"door": "owner", "export": owner.export, "id": id, "request": request}));
        self.owner.clone().expect("the owner door was not expected")
    }
}

fn call(doors: &Fake, platform: &str, id: &str, request: &str) -> Value {
    let text = invoke_with(doors, platform, id, request)
        .now_or_never()
        .expect("fake doors answer at once");
    serde_json::from_str(&text).expect("invoke answers JSON")
}

fn keys(value: &Value) -> Vec<&str> {
    let mut keys: Vec<&str> = value
        .as_object()
        .expect("an object")
        .keys()
        .map(String::as_str)
        .collect();
    keys.sort_unstable();
    keys
}

/// The installed CLI's envelopes for the same refusals (`ds … --output json`).
fn cli(text: &str) -> Value {
    serde_json::from_str(text).unwrap()
}
const CLI_MISSING: &str = r#"{"v":1,"command":"assets.tree","contract":2,"status":"error","error":{"class":"invalid_input","code":"missing_input","message":"`--project` is required","retryable":false,"remedy":"pass `--project <ds-project>`","next":["ds assets tree --help"]}}"#;
const CLI_UNKNOWN_FLAG: &str = r#"{"v":1,"command":"assets.tree","contract":2,"status":"error","error":{"class":"invalid_input","code":"unknown_flag","message":"`--bogus` is not an input of `ds assets tree`","retryable":false,"remedy":"run the command's help for its declared inputs","next":["ds assets tree --help"]}}"#;
const CLI_MISSING_SEVERAL: &str = r#"{"v":1,"command":"pm.task.create","contract":2,"status":"error","error":{"class":"invalid_input","code":"missing_input","message":"`--title` and `--project` are required","retryable":false,"remedy":"pass `--title <text>` and `--project <ds-project>`","next":["ds pm task create --help"],"detail":{"missing":["title","project"]}}}"#;
const CLI_SUCCESS_KEYS: [&str; 5] = ["command", "contract", "data", "status", "v"];
const CLI_ERROR_KEYS: [&str; 5] = ["command", "contract", "error", "status", "v"];

/// The envelope frame and error body are the CLI's: same members, same
/// version and the same class/code/retryable for the same refusal.
fn same_refusal(ours: &Value, theirs: &Value) {
    assert_eq!(keys(ours), CLI_ERROR_KEYS);
    assert_eq!(ours["v"], theirs["v"]);
    assert_eq!(ours["status"], theirs["status"]);
    assert_eq!(ours["command"], theirs["command"]);
    for field in ["class", "code", "retryable"] {
        assert_eq!(
            ours["error"][field], theirs["error"][field],
            "error.{field}"
        );
    }
    for field in ["message", "remedy"] {
        assert!(ours["error"][field].is_string(), "error.{field} is text");
    }
}

#[test]
fn every_in_page_export_has_exactly_one_invoke_decision() {
    let exports = manifest_exports();
    assert!(exports.len() > 90, "the manifest names its page owners");
    let mut decided: Vec<&str> = NATIVE_OWNERS
        .iter()
        .chain(NATIVE_OWNERS_TYPED)
        .map(|owner| owner.export)
        .chain(PAGE_OWNERS.iter().map(|owner| owner.export))
        .chain(UNBOUND.iter().map(|owner| owner.export))
        .collect();
    let count = decided.len();
    decided.sort_unstable();
    decided.dedup();
    assert_eq!(decided.len(), count, "an export has two decisions");
    let decided: BTreeSet<&str> = decided.into_iter().collect();
    let undecided: Vec<_> = exports.difference(&decided).collect();
    assert!(
        undecided.is_empty(),
        "exports with no invoke decision: {undecided:?}"
    );
    let stale: Vec<_> = decided.difference(&exports).collect();
    assert!(
        stale.is_empty(),
        "decisions for exports no row names: {stale:?}"
    );
    for owner in PAGE_OWNERS {
        let mut names: Vec<_> = owner.members.iter().map(|member| member.name).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(
            names.len(),
            owner.members.len(),
            "{} repeats a member",
            owner.export
        );
        let whole = owner
            .members
            .iter()
            .filter(|member| member.kind == Kind::Whole)
            .count();
        assert!(
            whole == 0 || owner.members.len() == 1,
            "{} mixes a whole request with members",
            owner.export
        );
    }
    let bound = exports.len() - UNBOUND.len();
    assert!(
        bound * 10 >= exports.len() * 8,
        "invoke carries most page owners ({bound}/{})",
        exports.len()
    );
}

#[test]
fn a_kernel_module_row_runs_its_typed_owner_and_answers_the_cli_success_envelope() {
    let snapshot = json!({
        "project_id": "p",
        "styles": {"master/poles": {"type": "line", "paint": {"line-width": 3}}},
        "style_editors": [{"style_ref": "master/poles", "style_target": "design", "layer_name": "poles",
            "available_fields": ["status"], "field_domains": {"status": {"type": "boolean"}},
            "label_model": {"type": "symbol", "paint": {}, "layout": {"text-field": ["get", "status"]}}}],
        "label_editor": {"field_part_types": ["literal", "field"]},
        "style_properties": {}
    });
    let request = json!({"operation": "list", "snapshot": snapshot, "limit": 10});
    let fake = Fake::default();
    let envelope = call(&fake, "web", "style.list", &request.to_string());
    assert_eq!(keys(&envelope), CLI_SUCCESS_KEYS);
    assert_eq!(envelope["v"], 1);
    assert_eq!(envelope["status"], "ok");
    assert_eq!(envelope["command"], "style.list");
    assert_eq!(envelope["contract"], CONTRACT);
    // The same owner function the `styleCatalog` export calls, byte for byte.
    let direct: Value = serde_json::from_str(
        &crate::style_plan::style_catalog_request(&serde_json::to_vec(&request).unwrap()).unwrap(),
    )
    .unwrap();
    assert_eq!(envelope["data"], direct);
    assert!(
        fake.asked.borrow().is_empty(),
        "a native owner opens no door"
    );

    // The sibling row the same export serves is not this one.
    let read =
        json!({"operation": "read", "snapshot": request["snapshot"], "reference": "master/poles"});
    let refused = call(&fake, "web", "style.list", &read.to_string());
    assert_eq!(refused["error"]["code"], CODE_REQUEST_MISMATCH);
    assert_eq!(
        refused["error"]["detail"]["performs"],
        json!(["style.read"])
    );
    assert_eq!(
        call(&fake, "web", "style.read", &read.to_string())["status"],
        "ok"
    );
    // The owner's own refusal of an undeclared member is an error envelope.
    let mut extra = request.clone();
    extra["map"] = json!({});
    let refused = call(&fake, "web", "style.list", &extra.to_string());
    assert_eq!(keys(&refused), CLI_ERROR_KEYS);
    assert_eq!(refused["error"]["class"], "failed");
}

#[test]
fn a_kernel_page_owner_takes_its_named_inputs_with_the_clis_refusals() {
    let fake = Fake {
        owner: Some(Ok(json!([{"path": "/", "count": 2}]))),
        ..Fake::default()
    };
    let envelope = call(&fake, "web", "assets.tree", r#"{"project":"p1"}"#);
    assert_eq!(keys(&envelope), CLI_SUCCESS_KEYS);
    assert_eq!(envelope["data"], json!([{"path": "/", "count": 2}]));
    assert_eq!(
        fake.asked.borrow()[0],
        json!({"door": "owner", "export": "assetsTree", "id": "assets.tree", "request": {"project": "p1"}})
    );

    // `ds assets tree --output json` and the same request through invoke.
    same_refusal(&call(&fake, "web", "assets.tree", "{}"), &cli(CLI_MISSING));
    let unknown = call(&fake, "web", "assets.tree", r#"{"project":"p1","bogus":1}"#);
    same_refusal(&unknown, &cli(CLI_UNKNOWN_FLAG));
    assert_eq!(unknown["error"]["detail"]["accepted"], json!(["project"]));
    // Several omissions at once, as `ds pm task create` names them.
    let several = admit_members(
        "governance.user.access.set",
        PAGE_OWNERS
            .iter()
            .find(|owner| owner.export == "governanceSetAccessState")
            .unwrap()
            .members,
        &Map::new(),
    )
    .unwrap_err();
    let several: Value = serde_json::from_str(&error_text("pm.task.create", &several)).unwrap();
    same_refusal(&several, &cli(CLI_MISSING_SEVERAL));
    assert_eq!(
        several["error"]["detail"]["missing"],
        json!(["email", "account_status", "reason"])
    );
    let typed = call(&fake, "web", "assets.tree", r#"{"project":7}"#);
    assert_eq!(typed["error"]["code"], "invalid_value");
    assert_eq!(
        fake.asked.borrow().len(),
        1,
        "a refused request opens no door"
    );

    // The owner's typed rejection keeps its code; the class follows from it.
    let signed_out = Fake {
        owner: Some(Err(from_rejection(&Rejection {
            code: Some("SIGNED_OUT".into()),
            message: Some("Request ignored because the session has ended.".into()),
            status: Some(401),
            signed_out: true,
            ..Rejection::default()
        }))),
        ..Fake::default()
    };
    let refused = call(&signed_out, "web", "feedback.list", "{}");
    assert_eq!(refused["error"]["class"], "unauthorized");
    assert_eq!(refused["error"]["code"], "signed_out");
    assert_eq!(refused["error"]["detail"]["status"], 401);
}

#[test]
fn a_shared_owner_runs_only_the_row_its_request_performs() {
    let fake = Fake {
        owner: Some(Ok(json!({"ok": true}))),
        ..Fake::default()
    };
    let read = json!({"action": "task_read", "ds_project": "p1", "task_id": "t1"});
    assert_eq!(
        call(&fake, "web", "pm.task.read", &read.to_string())["status"],
        "ok"
    );
    let refused = call(&fake, "web", "pm.task.create", &read.to_string());
    assert_eq!(refused["error"]["code"], CODE_REQUEST_MISMATCH);
    assert_eq!(refused["error"]["class"], "invalid_input");
    let unknown = call(&fake, "web", "pm.task.read", r#"{"action":"launch"}"#);
    assert_eq!(
        unknown["error"]["code"],
        crate::gateway_exchange::project_work::INPUT_INVALID
    );
    assert_eq!(fake.asked.borrow().len(), 1);
}

#[test]
fn a_network_unit_row_runs_on_the_pages_unit_door() {
    let fake = Fake {
        unit: Some(Ok(json!({"session": "s1"}))),
        ..Fake::default()
    };
    let envelope = call(&fake, "web", "grid.session", r#"{"open":{"model":"m"}}"#);
    assert_eq!(keys(&envelope), CLI_SUCCESS_KEYS);
    assert_eq!(envelope["command"], "grid.session");
    assert_eq!(envelope["data"], json!({"session": "s1"}));
    assert_eq!(
        fake.asked.borrow()[0],
        json!({"door": "unit", "id": "grid.session", "input": {"open": {"model": "m"}}})
    );
    // On the Desktop the same row waits for its unit loader.
    let pending = call(&Fake::default(), "desktop", "grid.session", "{}");
    assert_eq!(pending["error"]["code"], crate::capabilities::CODE_PENDING);
}

#[test]
fn a_core_row_opens_the_core_door_with_its_run_spec_and_answers_cores_envelope() {
    // Core's own envelope, with a member this Kernel has never seen.
    let core = r#"{"v":1,"command":"design.process.settings","contract":3,"status":"ok","data":{"catalog":[]},"later":{"added":true}}"#;
    let fake = Fake {
        core: Some(Ok(core.into())),
        ..Fake::default()
    };
    let text = invoke_with(
        &fake,
        "desktop",
        "design.process.settings",
        r#"{"project":"p1"}"#,
    )
    .now_or_never()
    .unwrap();
    assert_eq!(text, core, "Core's envelope reaches the caller unchanged");
    assert_eq!(
        fake.asked.borrow()[0],
        json!({"door": "core", "id": "design.process.settings", "project": "p1", "args": {"catalog": true}})
    );
    // Core's refusal is Core's envelope too, exactly as `ds` prints it.
    let refused = r#"{"v":1,"command":"design.process.settings","contract":3,"status":"error","error":{"class":"invalid_input","code":"unknown_flag","message":"`--bogus` is not an input of `ds design process settings`","retryable":false}}"#;
    let fake = Fake {
        core: Some(Ok(refused.into())),
        ..Fake::default()
    };
    assert_eq!(
        invoke_with(
            &fake,
            "desktop",
            "design.process.settings",
            r#"{"bogus":1}"#
        )
        .now_or_never()
        .unwrap(),
        refused
    );
    // An answer that is not this command's envelope is refused typed.
    for answer in [
        r#"{"status":"ok"}"#,
        r#"{"v":1,"command":"other","contract":1,"status":"ok","data":{}}"#,
    ] {
        let fake = Fake {
            core: Some(Ok(answer.into())),
            ..Fake::default()
        };
        let envelope = call(&fake, "desktop", "design.process.settings", "{}");
        assert_eq!(envelope["error"]["code"], CODE_CORE_INCOMPATIBLE);
    }
    // A door the page did not pass is the typed refusal.
    let fake = Fake {
        core: Some(Err(refusal(
            Class::Unavailable,
            CODE_DOOR_UNAVAILABLE,
            "no core door",
        ))),
        ..Fake::default()
    };
    let envelope = call(&fake, "desktop", "design.process.settings", "{}");
    assert_eq!(envelope["error"]["code"], CODE_DOOR_UNAVAILABLE);
    assert_eq!(envelope["error"]["class"], "unavailable");
    // The Desktop runs a Web-kernel row on Core: `assets.tree` reaches Core.
    let fake = Fake {
        core: Some(Ok(CLI_MISSING.into())),
        ..Fake::default()
    };
    assert_eq!(
        call(&fake, "desktop", "assets.tree", "{}"),
        cli(CLI_MISSING)
    );
}

#[test]
fn pending_and_discarded_rows_refuse_with_their_reason() {
    let fake = Fake::default();
    let pending = call(&fake, "web", "design.autoprocess.plan", "{}");
    assert_eq!(keys(&pending), CLI_ERROR_KEYS);
    assert_eq!(pending["command"], "design.autoprocess.plan");
    assert_eq!(pending["error"]["code"], crate::capabilities::CODE_PENDING);
    assert_eq!(pending["error"]["class"], "unavailable");
    assert_eq!(pending["error"]["retryable"], false);
    assert_eq!(pending["error"]["detail"]["reason"], "engineering_on_core");
    assert_eq!(
        pending["error"]["message"],
        "Engineering processing runs on DS Core for now: open this project in DS GridDesign Desktop, or use `ds`."
    );
    let discarded = call(&fake, "web", "account.connect", "{}");
    assert_eq!(
        discarded["error"]["code"],
        crate::capabilities::CODE_DISCARDED
    );
    assert_eq!(discarded["error"]["detail"]["reason"], "terminal_only");
    // A row's refusal comes before its request is read.
    assert_eq!(
        call(&fake, "web", "account.connect", "not json")["error"]["code"],
        crate::capabilities::CODE_DISCARDED
    );
    // A row whose owner this door does not carry yet is refused typed.
    let unbound = call(&fake, "web", "messaging.send", "{}");
    assert_eq!(unbound["error"]["code"], CODE_UNBOUND);
    assert_eq!(unbound["error"]["detail"]["export"], "messagingOutboxDrain");
    assert!(fake.asked.borrow().is_empty());
}

#[test]
fn an_unknown_id_and_a_malformed_request_refuse_before_any_door() {
    let fake = Fake::default();
    let unknown = call(&fake, "web", "pm.task.launch", "{}");
    assert_eq!(keys(&unknown), CLI_ERROR_KEYS);
    assert_eq!(unknown["command"], "pm.task.launch");
    assert_eq!(unknown["error"]["code"], CODE_UNKNOWN);
    assert_eq!(unknown["error"]["class"], "invalid_input");
    // An id that names nothing well-formed answers as `ds` itself, as the CLI does.
    assert_eq!(call(&fake, "web", "Not An Id", "{}")["command"], "ds");
    for request in ["", "{", "[1,2]", "\"text\""] {
        let malformed = call(&fake, "web", "assets.tree", request);
        assert_eq!(keys(&malformed), CLI_ERROR_KEYS, "{request}");
        assert_eq!(
            malformed["error"]["code"], CODE_REQUEST_INVALID,
            "{request}"
        );
        assert_eq!(malformed["error"]["class"], "invalid_input");
        assert_eq!(malformed["error"]["retryable"], false);
    }
    let core = call(&fake, "desktop", "design.process.settings", "[]");
    assert_eq!(core["error"]["code"], CODE_REQUEST_INVALID);
    let project = call(
        &fake,
        "desktop",
        "design.process.settings",
        r#"{"project":1}"#,
    );
    assert_eq!(project["error"]["code"], "invalid_value");
    assert!(fake.asked.borrow().is_empty());
}

#[test]
fn a_rejection_keeps_its_owners_code_and_takes_the_class_it_states() {
    let refusal = |rejection: Rejection| from_rejection(&rejection);
    let conflict = refusal(Rejection {
        code: Some("version_conflict".into()),
        message: Some("Changed since read.".into()),
        status: Some(409),
        ..Rejection::default()
    });
    assert_eq!(
        (conflict.class.as_str(), conflict.code.as_str()),
        ("conflict", "version_conflict")
    );
    let offline = refusal(Rejection {
        code: Some("NETWORK_OFFLINE".into()),
        message: Some("Offline.".into()),
        transient: true,
        retryable: Some(true),
        ..Rejection::default()
    });
    assert_eq!(
        (
            offline.class.as_str(),
            offline.code.as_str(),
            offline.retryable
        ),
        ("unavailable", "network_offline", true)
    );
    let unknown_outcome = refusal(Rejection {
        code: Some("REQUEST_TIMEOUT".into()),
        retryable: Some(true),
        outcome_unknown: true,
        ..Rejection::default()
    });
    assert!(
        !unknown_outcome.retryable,
        "a write whose outcome is unknown is never retried blind"
    );
    assert_eq!(
        unknown_outcome.detail,
        Some(json!({"outcome_unknown": true}))
    );
    let typed = refusal(Rejection {
        class: Some("failed".into()),
        code: Some("core_refused".into()),
        message: Some("No.".into()),
        remedy: Some("Fix it.".into()),
        next: vec!["ds core status".into()],
        ..Rejection::default()
    });
    assert_eq!(
        (typed.class.as_str(), typed.remedy.as_deref()),
        ("failed", Some("Fix it."))
    );
    let bare = refusal(Rejection::default());
    assert_eq!(
        (bare.class.as_str(), bare.code.as_str()),
        ("failed", CODE_FAILED)
    );
    let envelope: Value = serde_json::from_str(&error_text("feedback.list", &typed)).unwrap();
    assert_eq!(envelope["error"]["next"], json!(["ds core status"]));
}
