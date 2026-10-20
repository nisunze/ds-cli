use super::*;

const T1: &str = "11111111111111111111111111111111";
const T2: &str = "22222222222222222222222222222222";
const T3: &str = "33333333333333333333333333333333";
const T4: &str = "44444444444444444444444444444444";
const GIS: &str = "gisFolder:[\"kigali\",\"Survey\"]";

fn who(principal: Principal) -> Identity {
    Identity {
        principal,
        lane: "canary".into(),
        audience: "ds-prod".into(),
    }
}
fn alice() -> Identity {
    who(Principal::Account("alice".into()))
}
fn bob() -> Identity {
    who(Principal::Account("bob".into()))
}
fn db() -> Connection {
    Connection::open_in_memory().unwrap()
}
fn run(connection: &Connection, identity: &Identity, value: Value) -> Result<Value, String> {
    let operation = serde_json::from_value(value).map_err(|e| e.to_string())?;
    execute(connection, Input { identity, bytes: None }, operation).map(|outcome| Value::Object(outcome.value))
}
fn connect(connection: &Connection, identity: &Identity, project: &str, key: &str, token: &str, now: u64) -> Value {
    run(
        connection,
        identity,
        json!({"action":"connect","project":project,"logical_key":key,"folder_name":"Survey","token":token,"now":now}),
    )
    .unwrap()
}
fn lookup(connection: &Connection, identity: &Identity, project: &str, key: &str) -> Value {
    run(connection, identity, json!({"action":"lookup","targets":[{"project":project,"logical_key":key}]})).unwrap()
        ["connections"][0]
        .clone()
}

#[test]
fn a_connection_is_found_only_in_its_project_and_names_its_token() {
    let connection = db();
    let made = connect(&connection, &alice(), "kigali", GIS, T1, 10);
    assert_eq!(made["drop"], json!([]));
    assert_eq!(made["connection"]["kind"], "gis_folder");
    let found = lookup(&connection, &alice(), "kigali", GIS);
    assert_eq!(found["token"], T1);
    assert_eq!(found["folder_name"], "Survey");
    assert!(lookup(&connection, &alice(), "musanze", GIS).is_null());
}

#[test]
fn reconnecting_replaces_the_token_and_drops_the_old_one() {
    let connection = db();
    connect(&connection, &alice(), "kigali", GIS, T1, 10);
    let again = connect(&connection, &alice(), "kigali", GIS, T2, 20);
    assert_eq!(again["drop"], json!([T1]));
    assert_eq!(again["connection"]["created_at_ms"], 10);
    assert_eq!(again["connection"]["updated_at_ms"], 20);
    // The same key under another project is a new connection in time.
    let moved = connect(&connection, &alice(), "musanze", GIS, T3, 30);
    assert_eq!(moved["drop"], json!([T2]));
    assert_eq!(moved["connection"]["created_at_ms"], 30);
    // Re-stating the current token drops nothing.
    assert_eq!(connect(&connection, &alice(), "musanze", GIS, T3, 40)["drop"], json!([]));
}

#[test]
fn a_token_names_one_connection_and_identities_never_share_rows() {
    let connection = db();
    connect(&connection, &alice(), "kigali", GIS, T1, 10);
    let reused = run(
        &connection,
        &bob(),
        json!({"action":"connect","project":"kigali","logical_key":GIS,"folder_name":"Survey","token":T1,"now":1}),
    )
    .unwrap_err();
    assert!(reused.starts_with(INVALID), "{reused}");
    assert!(lookup(&connection, &bob(), "kigali", GIS).is_null());
    let anonymous = who(Principal::Anonymous);
    assert!(lookup(&connection, &anonymous, "kigali", GIS).is_null());
    connect(&connection, &anonymous, "local", GIS, T2, 1);
    assert_eq!(lookup(&connection, &anonymous, "local", GIS)["token"], T2);
    assert!(lookup(&connection, &alice(), "local", GIS).is_null());
}

#[test]
fn the_sweep_drops_only_unreferenced_gis_folders_and_keeps_pls_links_and_export_workspaces() {
    let connection = db();
    let old_gis = "gisFolder:[\"kigali\",\"Old\"]";
    connect(&connection, &alice(), "kigali", GIS, T1, 1);
    connect(&connection, &alice(), "kigali", old_gis, T2, 1);
    connect(&connection, &alice(), "kigali", "plsFolder:kigali:model-1", T3, 1);
    connect(&connection, &alice(), "__ds_grid_export__", "grid-export:pls-workspace", T4, 1);
    let referenced: BTreeSet<String> = [GIS.to_owned()].into();
    let owner = alice().owner();
    assert_eq!(retain_referenced_gis(&connection, &owner, &referenced).unwrap(), vec![T2.to_owned()]);
    assert!(lookup(&connection, &alice(), "kigali", old_gis).is_null());
    assert_eq!(lookup(&connection, &alice(), "kigali", GIS)["token"], T1);
    assert_eq!(lookup(&connection, &alice(), "kigali", "plsFolder:kigali:model-1")["token"], T3);
    assert_eq!(lookup(&connection, &alice(), "__ds_grid_export__", "grid-export:pls-workspace")["token"], T4);
    // Another identity's connections are never swept by this one.
    connect(&connection, &bob(), "kigali", old_gis, T2, 1);
    retain_referenced_gis(&connection, &owner, &BTreeSet::new()).unwrap();
    assert_eq!(lookup(&connection, &bob(), "kigali", old_gis)["token"], T2);
}

#[test]
fn only_tokens_no_identity_names_are_unreferenced() {
    let connection = db();
    connect(&connection, &alice(), "kigali", GIS, T1, 1);
    connect(&connection, &bob(), "kigali", GIS, T2, 1);
    let present = vec![T1.to_owned(), T2.to_owned(), T3.to_owned(), T3.to_owned()];
    assert_eq!(unreferenced_tokens(&connection, &present).unwrap(), vec![T3.to_owned()]);
    assert!(unreferenced_tokens(&connection, &["nothex".to_owned()]).is_err());
}

fn legacy_rows() -> Value {
    json!([
        // alice's own scoped row.
        {"key": "[\"alice\",\"gisFolder:a\"]", "owner_uid": "alice", "logical_handle_key": "gisFolder:a",
         "ds_project": "kigali", "folder_name": "A", "created_at": 5.0, "updated_at": 6.0, "directory": true, "token": T1},
        // bob's row: never alice's.
        {"key": "[\"bob\",\"gisFolder:b\"]", "owner_uid": "bob", "logical_handle_key": "gisFolder:b",
         "ds_project": "kigali", "folder_name": "B", "directory": true, "token": T2},
        // A row from before account scoping: claimable by an account.
        {"key": "plsFolder:kigali:m", "ds_project": "kigali", "folder_name": "P", "directory": true, "token": T3},
        // A scoped row whose physical key does not match its fields.
        {"key": "forged", "owner_uid": "alice", "logical_handle_key": "gisFolder:c",
         "ds_project": "kigali", "folder_name": "C", "directory": true, "token": T4},
    ])
}

#[test]
fn the_import_admits_this_accounts_rows_and_unscoped_rows_once_with_a_receipt() {
    let connection = db();
    let legacy = run(&connection, &alice(), json!({"action":"legacy"})).unwrap();
    assert_eq!(legacy["import_needed"], true);
    assert_eq!(legacy["legacy"], json!({"database":"ds_web_store","store":"folder_connections"}));
    let imported = run(&connection, &alice(), json!({"action":"import","rows":legacy_rows(),"now":100})).unwrap();
    assert_eq!(imported["imported"], true);
    assert_eq!(imported["receipt"]["source"], LEGACY_SOURCE);
    assert_eq!(imported["receipt"]["count"], 2);
    assert_eq!(imported["receipt"]["skipped"], 2);
    assert_eq!(imported["drop"], json!([T2, T4]));
    let own = lookup(&connection, &alice(), "kigali", "gisFolder:a");
    assert_eq!(own["token"], T1);
    assert_eq!(own["created_at_ms"], 5);
    assert_eq!(lookup(&connection, &alice(), "kigali", "plsFolder:kigali:m")["kind"], "pls_folder");
    assert_eq!(run(&connection, &alice(), json!({"action":"legacy"})).unwrap()["import_needed"], false);

    let again = run(&connection, &alice(), json!({"action":"import","rows":legacy_rows(),"now":200})).unwrap();
    assert_eq!(again["imported"], false);
    assert_eq!(again["receipt"], imported["receipt"]);
    assert_eq!(again["drop"], json!([T1, T2, T3, T4]));
}

#[test]
fn an_import_keeps_newer_connections_and_an_anonymous_session_claims_nothing() {
    let connection = db();
    let newer = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    connect(&connection, &alice(), "kigali", "gisFolder:a", newer, 50);
    let imported = run(&connection, &alice(), json!({"action":"import","rows":legacy_rows(),"now":100})).unwrap();
    assert_eq!(imported["receipt"]["count"], 1);
    assert_eq!(lookup(&connection, &alice(), "kigali", "gisFolder:a")["token"], newer);
    assert_eq!(imported["drop"], json!([T1, T2, T4]));

    let fresh = db();
    let anonymous = who(Principal::Anonymous);
    let none = run(&fresh, &anonymous, json!({"action":"import","rows":legacy_rows(),"now":1})).unwrap();
    assert_eq!(none["receipt"]["count"], 0);
    assert_eq!(none["drop"], json!([T1, T2, T3, T4]));
}

#[test]
fn malformed_connections_refuse_and_carry_no_bytes() {
    let connection = db();
    for value in [
        json!({"action":"connect","project":"kigali","logical_key":GIS,"folder_name":"S","token":"short","now":1}),
        json!({"action":"connect","project":"a/b","logical_key":GIS,"folder_name":"S","token":T1,"now":1}),
        json!({"action":"connect","project":"kigali","logical_key":" padded","folder_name":"S","token":T1,"now":1}),
        json!({"action":"connect","project":"kigali","logical_key":GIS,"folder_name":"","token":T1,"now":1}),
        json!({"action":"connect","project":"kigali","logical_key":GIS,"folder_name":"S","token":T1,"now":9007199254740992u64}),
    ] {
        let error = run(&connection, &alice(), value.clone()).unwrap_err();
        assert!(error.starts_with(INVALID), "{value}: {error}");
    }
    let identity = alice();
    let refused = execute(
        &connection,
        Input { identity: &identity, bytes: Some(vec![0]) },
        serde_json::from_value(json!({"action":"legacy"})).unwrap(),
    )
    .unwrap_err();
    assert!(refused.starts_with(INVALID), "{refused}");
}
