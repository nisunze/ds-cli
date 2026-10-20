use super::*;
use std::collections::BTreeMap;

const APP: &str = "1:1234567890:web:abc123";
const N1: &str = "0123456789abcdef0123456789abcdef";
const N2: &str = "fedcba9876543210fedcba9876543210";
const N3: &str = "00112233445566778899aabbccddeeff";
const HASH: &str = "aa00000000000000000000000000000000000000000000000000000000000000";
const EPOCH: &str = "e0000000000000000000000000000001";
const DELETE: Effect = Effect::DeleteFile {
    file: EffectFile::PushProjection,
};

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
fn anonymous() -> Identity {
    who(Principal::Anonymous)
}
fn db() -> Connection {
    Connection::open_in_memory().unwrap()
}
fn op(value: Value) -> Operation {
    serde_json::from_value(value).unwrap()
}
/// The answer's fields with its after-effects under `effects`.
fn attempt(connection: &Connection, identity: &Identity, value: Value) -> Result<Value, String> {
    let outcome = execute(connection, Input { identity, bytes: None }, op(value))?;
    let mut answer = Value::Object(outcome.value);
    answer["effects"] = serde_json::to_value(&outcome.after).unwrap();
    answer["wrote"] = outcome.wrote.into();
    Ok(answer)
}
fn run(connection: &Connection, identity: &Identity, value: Value) -> Value {
    attempt(connection, identity, value).unwrap()
}
fn generation(answer: &Value) -> String {
    answer["record"]["generation"].as_str().unwrap().to_owned()
}
fn after(answer: &Value) -> Vec<Effect> {
    serde_json::from_value(answer["effects"].clone()).unwrap()
}
fn opt_in(connection: &Connection, identity: &Identity, nonce: &str) -> Value {
    run(connection, identity, json!({"action":"opt_in","app_id":APP,"nonce":nonce,"now":10}))
}
fn commit(connection: &Connection, identity: &Identity, generation: &str, fid: &str) -> Value {
    run(
        connection,
        identity,
        json!({"action":"commit_registration","app_id":APP,"generation":generation,
               "fid":fid,"installation_hash":HASH,"epoch":EPOCH}),
    )
}
/// Opt alice in and activate a registration; answers the commit.
fn activate(connection: &Connection) -> Value {
    run(connection, &alice(), json!({"action":"observe_session","app_id":APP,"nonce":N1}));
    let opted = opt_in(connection, &alice(), N2);
    commit(connection, &alice(), &generation(&opted), "fid-1")
}

#[test]
fn the_session_owner_is_the_bound_principal_and_anonymous_owns_nothing() {
    let connection = db();
    let seen = run(&connection, &alice(), json!({"action":"observe_session","app_id":APP,"nonce":N1}));
    assert_eq!(seen["record"]["registrable"], false);
    assert_eq!(seen["record"]["enabled"], false);
    assert_eq!(seen["wrote"], true);
    let read = run(&connection, &alice(), json!({"action":"read","app_id":APP}));
    assert_eq!(read["record"]["generation"], seen["record"]["generation"]);
    assert_eq!(read["import_needed"], true);
    assert_eq!(read["wrote"], false);
    assert_eq!(after(&read), vec![]);
    // A caller cannot state an owner: no operation has an owner field.
    assert!(serde_json::from_value::<Operation>(
        json!({"action":"opt_in","app_id":APP,"nonce":N1,"now":1,"owner":"mallory"})
    )
    .is_err());
    let refused = attempt(&connection, &anonymous(), json!({"action":"opt_in","app_id":APP,"nonce":N2,"now":1}))
        .unwrap_err();
    assert!(refused.starts_with(INVALID), "{refused}");
}

#[test]
fn an_activated_registration_projects_exactly_its_owner_epoch_and_generation() {
    let connection = db();
    let committed = activate(&connection);
    assert_eq!(committed["applied"], true);
    let active = &committed["record"]["active"];
    assert_eq!(active["owner"], "alice");
    assert_eq!(active["epoch"], EPOCH);
    let effects = after(&committed);
    let [Effect::WriteFile { file, contents }] = effects.as_slice() else {
        panic!("an activation writes the projection: {effects:?}");
    };
    assert_eq!(*file, EffectFile::PushProjection);
    let projection: Value = serde_json::from_str(contents).unwrap();
    assert_eq!(
        projection,
        json!({"schema":PROJECTION_SCHEMA,"app_id":APP,"owner":"alice","epoch":EPOCH,
               "generation":committed["record"]["generation"]})
    );
    let matching = run(&connection, &alice(), json!({"action":"read","app_id":APP,"epoch":EPOCH}));
    assert_eq!(matching["record"]["active"]["fid"], "fid-1");
    let other = run(
        &connection,
        &alice(),
        json!({"action":"read","app_id":APP,"epoch":"e0000000000000000000000000000002"}),
    );
    assert!(other["record"]["active"].is_null());
    // Another account never sees alice's registration.
    let bob_read = run(&connection, &bob(), json!({"action":"read","app_id":APP}));
    assert!(bob_read["record"]["active"].is_null());
    assert_eq!(bob_read["record"]["enabled"], false);
}

#[test]
fn an_account_switch_rotates_the_generation_and_fences_the_older_attempt() {
    let connection = db();
    run(&connection, &alice(), json!({"action":"observe_session","app_id":APP,"nonce":N1}));
    let attempt_generation = generation(&opt_in(&connection, &alice(), N2));
    let check = json!({"action":"check","app_id":APP,"generation":attempt_generation,"expect":"registering"});
    assert_eq!(run(&connection, &alice(), check.clone())["current"], true);

    let switched = run(&connection, &bob(), json!({"action":"observe_session","app_id":APP,"nonce":N3}));
    assert_ne!(generation(&switched), attempt_generation);
    assert_eq!(after(&switched), vec![DELETE]);
    assert_eq!(run(&connection, &alice(), check.clone())["current"], false);
    assert_eq!(run(&connection, &bob(), check)["current"], false);
    let late = commit(&connection, &alice(), &attempt_generation, "fid-1");
    assert_eq!(late["applied"], false);
    assert!(late["record"]["active"].is_null());
    // Confirming the same owner changes nothing, generation included.
    let again = run(&connection, &bob(), json!({"action":"observe_session","app_id":APP,"nonce":N1}));
    assert_eq!(generation(&again), generation(&switched));
}

#[test]
fn signing_out_ends_the_registration_without_an_account() {
    let connection = db();
    activate(&connection);
    let out = run(&connection, &anonymous(), json!({"action":"observe_session","app_id":APP,"nonce":N3}));
    assert_eq!(after(&out), vec![DELETE]);
    let back = run(&connection, &alice(), json!({"action":"observe_session","app_id":APP,"nonce":N1}));
    // The opt-in survives a sign-out; the registration does not.
    assert_eq!(back["record"]["enabled"], true);
    assert_eq!(back["record"]["registrable"], true);
    assert!(back["record"]["active"].is_null());
}

#[test]
fn disabling_answers_the_registration_to_revoke_and_a_late_disable_is_stale() {
    let connection = db();
    activate(&connection);
    let disabled = run(&connection, &alice(), json!({"action":"disable","app_id":APP,"nonce":N3,"now":20}));
    assert_eq!(disabled["stale"], false);
    assert_eq!(disabled["previous"]["fid"], "fid-1");
    assert_eq!(disabled["previous"]["installation_hash"], HASH);
    assert_eq!(disabled["record"]["enabled"], false);
    let tombstone = json!({"action":"check","app_id":APP,"generation":generation(&disabled),"expect":"disabled"});
    assert_eq!(run(&connection, &alice(), tombstone.clone())["current"], true);
    assert_eq!(after(&disabled), vec![DELETE]);

    run(&connection, &bob(), json!({"action":"observe_session","app_id":APP,"nonce":N1}));
    assert_eq!(run(&connection, &alice(), tombstone)["current"], false);
    let late = run(&connection, &alice(), json!({"action":"disable","app_id":APP,"nonce":N2,"now":30}));
    assert_eq!(late["stale"], true);
    assert!(late["previous"].is_null());
}

#[test]
fn clearing_a_fid_ends_only_that_registration_and_clear_all_forgets_the_profile() {
    let connection = db();
    activate(&connection);
    let other = run(&connection, &alice(), json!({"action":"clear_fid","app_id":APP,"fid":"fid-2","nonce":N3}));
    assert_eq!(other["record"]["active"]["fid"], "fid-1");
    assert!(matches!(after(&other).as_slice(), [Effect::WriteFile { .. }]));
    let same = run(&connection, &alice(), json!({"action":"clear_fid","app_id":APP,"fid":"fid-1","nonce":N3}));
    assert!(same["record"]["active"].is_null());
    let cleared = run(&connection, &alice(), json!({"action":"clear_all"}));
    assert_eq!(after(&cleared), vec![DELETE]);
    assert!(run(&connection, &alice(), json!({"action":"read","app_id":APP}))["record"].is_null());
}

#[test]
fn every_write_but_activation_deletes_the_projection_before_its_transaction() {
    for (value, expected) in [
        (json!({"action":"read","app_id":APP}), vec![]),
        (json!({"action":"check","app_id":APP,"generation":N1,"expect":"registering"}), vec![]),
        (json!({"action":"legacy","app_id":APP}), vec![]),
        (
            json!({"action":"commit_registration","app_id":APP,"generation":N1,"fid":"f","installation_hash":HASH,"epoch":EPOCH}),
            vec![],
        ),
        (json!({"action":"observe_session","app_id":APP,"nonce":N1}), vec![DELETE]),
        (json!({"action":"opt_in","app_id":APP,"nonce":N1,"now":1}), vec![DELETE]),
        (json!({"action":"disable","app_id":APP,"nonce":N1,"now":1}), vec![DELETE]),
        (json!({"action":"clear_fid","app_id":APP,"fid":"f","nonce":N1}), vec![DELETE]),
        (json!({"action":"clear_all"}), vec![DELETE]),
        (json!({"action":"import","app_id":APP,"legacy":null,"nonce":N1,"now":1}), vec![DELETE]),
    ] {
        assert_eq!(effects_before(&op(value.clone())), expected, "{value}");
    }
}

/// A host that orders effects as the storage worker must: before-effects,
/// then one transaction, then after-effects on commit only.
struct Host {
    connection: Connection,
    opfs: BTreeMap<&'static str, String>,
    /// The projection observed while each transaction was open.
    during: Vec<Option<String>>,
}
impl Host {
    fn new() -> Self {
        Self {
            connection: db(),
            opfs: BTreeMap::new(),
            during: vec![],
        }
    }
    fn apply(&mut self, effects: Vec<Effect>) {
        for effect in effects {
            match effect {
                Effect::DeleteFile { file } => {
                    self.opfs.remove(file.path());
                }
                Effect::WriteFile { file, contents } => {
                    self.opfs.insert(file.path(), contents);
                }
            }
        }
    }
    fn run(&mut self, identity: &Identity, value: Value, crash: bool) -> Result<Value, String> {
        let operation = op(value);
        self.apply(effects_before(&operation));
        let transaction = self.connection.transaction().unwrap();
        let outcome = execute(&transaction, Input { identity, bytes: None }, operation);
        self.during.push(self.opfs.get(EffectFile::PushProjection.path()).cloned());
        match outcome {
            Ok(outcome) if !crash => {
                transaction.commit().unwrap();
                self.apply(outcome.after);
                Ok(Value::Object(outcome.value))
            }
            Ok(_) => Err("crashed before commit".into()),
            Err(error) => Err(error),
        }
    }
    fn projection(&self) -> Option<Value> {
        self.opfs
            .get(EffectFile::PushProjection.path())
            .map(|text| serde_json::from_str(text).unwrap())
    }
    fn activate(&mut self, nonce: &str, fid: &str) {
        let opted = self
            .run(&alice(), json!({"action":"opt_in","app_id":APP,"nonce":nonce,"now":1}), false)
            .unwrap();
        self.run(
            &alice(),
            json!({"action":"commit_registration","app_id":APP,"generation":generation(&opted),
                   "fid":fid,"installation_hash":HASH,"epoch":EPOCH}),
            false,
        )
        .unwrap();
    }
}

#[test]
fn disable_and_clear_remove_the_projection_before_commit_and_activation_writes_after() {
    let mut host = Host::new();
    host.run(&alice(), json!({"action":"observe_session","app_id":APP,"nonce":N1}), false).unwrap();
    // Activation: nothing is projected while its transaction is open.
    host.activate(N2, "fid-1");
    assert_eq!(host.during.last().unwrap(), &None);
    assert_eq!(host.projection().unwrap()["epoch"], EPOCH);

    // Disable: the projection is gone before the transaction opens.
    host.run(&alice(), json!({"action":"disable","app_id":APP,"nonce":N3,"now":2}), false).unwrap();
    assert_eq!(host.during.last().unwrap(), &None);
    assert_eq!(host.projection(), None);

    host.activate(N1, "fid-2");
    assert!(host.projection().is_some());
    host.run(&alice(), json!({"action":"clear_all"}), false).unwrap();
    assert_eq!(host.during.last().unwrap(), &None);
    assert_eq!(host.projection(), None);
}

#[test]
fn a_crash_inside_a_deactivating_transaction_leaves_the_projection_absent_not_stale() {
    let mut host = Host::new();
    host.run(&alice(), json!({"action":"observe_session","app_id":APP,"nonce":N1}), false).unwrap();
    host.activate(N2, "fid-1");
    assert!(host.projection().is_some());
    // The account switch crashes before its commit: the record still holds
    // alice's registration, but the service worker already reads nothing.
    host.run(&bob(), json!({"action":"observe_session","app_id":APP,"nonce":N3}), true).unwrap_err();
    assert_eq!(host.projection(), None);
    let held = run(&host.connection, &alice(), json!({"action":"read","app_id":APP}));
    assert_eq!(held["record"]["active"]["fid"], "fid-1");
    // The next committed write re-states the projection from the record.
    host.run(&alice(), json!({"action":"observe_session","app_id":APP,"nonce":N1}), false).unwrap();
    assert_eq!(host.projection().unwrap()["owner"], "alice");
}

#[test]
fn an_activation_that_crashes_before_commit_projects_nothing() {
    let mut host = Host::new();
    host.run(&alice(), json!({"action":"observe_session","app_id":APP,"nonce":N1}), false).unwrap();
    let opted = host
        .run(&alice(), json!({"action":"opt_in","app_id":APP,"nonce":N2,"now":1}), false)
        .unwrap();
    host.run(
        &alice(),
        json!({"action":"commit_registration","app_id":APP,"generation":generation(&opted),
               "fid":"fid-1","installation_hash":HASH,"epoch":EPOCH}),
        true,
    )
    .unwrap_err();
    assert_eq!(host.projection(), None);
    assert!(run(&host.connection, &alice(), json!({"action":"read","app_id":APP}))["record"]["active"].is_null());
}

fn legacy(opt_in: Option<&str>) -> Value {
    json!({
        "key": format!("app:{APP}"), "schema": 1, "generation": N1,
        "sessionOwnerUid": "alice", "optInOwnerUid": opt_in, "activeOwnerUid": "alice",
        "fid": "fid-old", "installationHash": HASH, "epoch": EPOCH,
    })
}
fn import(connection: &Connection, identity: &Identity, legacy: Value, now: u64) -> Value {
    run(
        connection,
        identity,
        json!({"action":"import","app_id":APP,"legacy":legacy,"nonce":N2,"now":now}),
    )
}

#[test]
fn the_retired_record_is_named_by_the_kernel() {
    let named = run(&db(), &alice(), json!({"action":"legacy","app_id":APP}));
    assert_eq!(
        named["legacy"],
        json!({"database":"ds-messaging-push","store":"state","keys":[format!("app:{APP}")]})
    );
}

#[test]
fn the_import_moves_only_this_accounts_opt_in_once_with_a_receipt() {
    let connection = db();
    let imported = import(&connection, &alice(), legacy(Some("alice")), 1000);
    assert_eq!(imported["imported"], true);
    let receipt = &imported["receipt"];
    assert_eq!(receipt["store"], STORE);
    assert_eq!(receipt["source"], LEGACY_SOURCE);
    assert_eq!(receipt["count"], 1);
    assert_eq!(receipt["skipped"], 0);
    assert_eq!(receipt["sha256"], sha256(legacy(Some("alice")).to_string().as_bytes()));
    // The opt-in moved; the old registration did not.
    assert_eq!(imported["record"]["enabled"], true);
    assert_eq!(imported["record"]["registrable"], true);
    assert!(imported["record"]["active"].is_null());
    assert_eq!(after(&imported), vec![DELETE]);
    assert_eq!(run(&connection, &alice(), json!({"action":"read","app_id":APP}))["import_needed"], false);

    let again = import(&connection, &alice(), legacy(Some("alice")), 2000);
    assert_eq!(again["imported"], false);
    assert_eq!(again["receipt"], *receipt);
    assert_eq!(generation(&again), generation(&imported));

    // Bob's own import of a record alice opted into moves nothing.
    let bobs = import(&connection, &bob(), legacy(Some("alice")), 3000);
    assert_eq!(bobs["receipt"]["count"], 0);
    assert_eq!(bobs["receipt"]["skipped"], 1);
    // The anonymous principal imports once too, and adopts nothing.
    let anonymous_import = import(&db(), &anonymous(), legacy(Some("alice")), 1);
    assert_eq!(anonymous_import["receipt"]["count"], 0);
}

#[test]
fn an_import_never_overrides_a_decision_made_here_and_admits_only_a_well_formed_record() {
    let connection = db();
    opt_in(&connection, &alice(), N2);
    run(&connection, &alice(), json!({"action":"disable","app_id":APP,"nonce":N3,"now":11}));
    // alice disabled push here: the retired opt-in never returns.
    assert_eq!(run(&connection, &alice(), json!({"action":"read","app_id":APP}))["import_needed"], false);
    let held = import(&connection, &alice(), legacy(Some("alice")), 12);
    assert_eq!(held["imported"], false);
    assert_eq!(held["receipt"]["count"], 0);
    assert_eq!(held["receipt"]["sha256"], sha256(b"[]"));
    assert_eq!(held["record"]["enabled"], false);

    // Another account's held opt-in is never replaced by an import.
    let shared = db();
    opt_in(&shared, &bob(), N1);
    let alices = import(&shared, &alice(), legacy(Some("alice")), 1);
    assert_eq!(alices["receipt"]["count"], 0);

    let fresh = db();
    let mut broken = legacy(Some("alice"));
    broken["epoch"] = json!("not-hex");
    assert_eq!(import(&fresh, &alice(), broken, 1)["receipt"]["skipped"], 1);
    let none = import(&db(), &alice(), Value::Null, 1);
    assert_eq!(none["receipt"]["count"], 0);
    assert_eq!(none["receipt"]["skipped"], 0);
}

#[test]
fn malformed_inputs_refuse_and_carry_no_bytes() {
    let connection = db();
    for value in [
        json!({"action":"observe_session","app_id":APP,"nonce":"short"}),
        json!({"action":"observe_session","app_id":" padded","nonce":N1}),
        json!({"action":"read","app_id":APP,"epoch":"E0000000000000000000000000000001"}),
        json!({"action":"commit_registration","app_id":APP,"generation":N1,"fid":"has space","installation_hash":HASH,"epoch":EPOCH}),
        json!({"action":"commit_registration","app_id":APP,"generation":N1,"fid":"f","installation_hash":"aa","epoch":EPOCH}),
        json!({"action":"opt_in","app_id":APP,"nonce":N1,"now":9007199254740992u64}),
    ] {
        let error = attempt(&connection, &alice(), value.clone()).unwrap_err();
        assert!(error.starts_with(INVALID), "{value}: {error}");
    }
    let identity = alice();
    let bytes = execute(
        &connection,
        Input { identity: &identity, bytes: Some(vec![1]) },
        op(json!({"action":"read","app_id":APP})),
    )
    .unwrap_err();
    assert!(bytes.starts_with(INVALID), "{bytes}");
}

#[test]
fn profiles_on_other_lanes_or_audiences_never_share_a_record() {
    let connection = db();
    activate(&connection);
    let mut stable = alice();
    stable.lane = "stable".into();
    assert!(run(&connection, &stable, json!({"action":"read","app_id":APP}))["record"].is_null());
    let mut other = alice();
    other.audience = "ds-dev".into();
    assert!(run(&connection, &other, json!({"action":"read","app_id":APP}))["record"].is_null());
}

#[test]
fn a_reused_nonce_still_rotates_the_generation() {
    let connection = db();
    let first = opt_in(&connection, &alice(), N1);
    let second = opt_in(&connection, &alice(), N1);
    assert_ne!(generation(&first), generation(&second));
}
