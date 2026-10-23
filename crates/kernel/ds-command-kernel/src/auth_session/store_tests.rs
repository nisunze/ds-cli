use super::*;
use crate::durable_store::{self, Call, Principal, Request, NOT_CORE};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};

const S1: &str = "0123456789abcdef0123456789abcdef";
const S2: &str = "fedcba9876543210fedcba9876543210";
const STATE: &str = "00112233445566778899aabbccddeeff";
const NONCE: &str = "ffeeddccbbaa99887766554433221100";

fn who(principal: Principal, audience: &str) -> Identity {
    Identity {
        principal,
        lane: "canary".into(),
        audience: audience.into(),
    }
}

fn anonymous() -> Identity {
    who(Principal::Anonymous, "ds-test")
}

fn alice() -> Identity {
    who(Principal::Account("alice".into()), "ds-test")
}

fn db() -> Connection {
    Connection::open_in_memory().unwrap()
}

fn record(uid: &str, session: &str, mark: &str) -> Record {
    let token = format!(
        "{}.{}.sig",
        URL_SAFE_NO_PAD.encode(br#"{"alg":"RS256"}"#),
        URL_SAFE_NO_PAD.encode(format!(r#"{{"sub":"{uid}","mark":"{mark}"}}"#))
    );
    Record {
        schema: super::super::SCHEMA.into(),
        session: session.into(),
        user: super::super::AuthUser {
            uid: uid.into(),
            email: format!("{uid}@example.test"),
            display_name: None,
            photo_url: None,
        },
        id_token: token,
        refresh_token: format!("refresh-{uid}-{mark}"),
        expires_at_epoch_seconds: 1_800_003_600,
        obtained_at_epoch_seconds: 1_800_000_000,
    }
}

fn run(
    connection: &Connection,
    identity: &Identity,
    operation: Operation,
) -> Result<(Value, bool), String> {
    let outcome = execute(
        connection,
        Input {
            identity,
            bytes: None,
        },
        operation,
    )?;
    Ok((Value::Object(outcome.value), outcome.wrote))
}

fn held(connection: &Connection, identity: &Identity) -> Option<Record> {
    let (answer, wrote) = run(connection, identity, Operation::Read).unwrap();
    assert!(!wrote);
    serde_json::from_value(answer["session"].clone()).unwrap()
}

fn redirect(state: &str) -> Redirect {
    Redirect {
        state: state.into(),
        nonce: NONCE.into(),
        redirect_uri: "https://app.example/auth/google".into(),
        return_path: "/".into(),
        created_at_epoch_seconds: 1_800_000_000,
    }
}

#[test]
fn a_session_round_trips_and_belongs_to_the_profile_not_an_account() {
    let connection = db();
    assert_eq!(held(&connection, &anonymous()), None);
    let (answer, wrote) = run(
        &connection,
        &anonymous(),
        Operation::SignIn {
            record: record("a", S1, "one"),
        },
    )
    .unwrap();
    assert!(wrote);
    assert_eq!(answer["session"], S1);
    // Read before anyone is known, and by the account it names alike.
    assert_eq!(
        held(&connection, &anonymous()),
        Some(record("a", S1, "one"))
    );
    assert_eq!(held(&connection, &alice()), Some(record("a", S1, "one")));
    // Another lane or audience is another profile.
    assert_eq!(
        held(&connection, &who(Principal::Anonymous, "ds-other")),
        None
    );
    let other_lane = Identity {
        lane: "production".into(),
        ..anonymous()
    };
    assert_eq!(held(&connection, &other_lane), None);
}

#[test]
fn a_refresh_replaces_only_its_own_session() {
    let connection = db();
    run(
        &connection,
        &anonymous(),
        Operation::SignIn {
            record: record("a", S1, "one"),
        },
    )
    .unwrap();
    run(
        &connection,
        &anonymous(),
        Operation::Refresh {
            record: record("a", S1, "two"),
        },
    )
    .unwrap();
    assert_eq!(
        held(&connection, &anonymous()),
        Some(record("a", S1, "two"))
    );
    // Another tab signed in as another account: the old session's refresh is refused.
    run(
        &connection,
        &anonymous(),
        Operation::SignIn {
            record: record("b", S2, "one"),
        },
    )
    .unwrap();
    let refused = run(
        &connection,
        &anonymous(),
        Operation::Refresh {
            record: record("a", S1, "three"),
        },
    )
    .unwrap_err();
    assert!(refused.starts_with(ENDED), "{refused}");
    assert_eq!(
        held(&connection, &anonymous()),
        Some(record("b", S2, "one"))
    );
}

#[test]
fn a_sign_out_removes_only_its_own_session_and_a_later_refresh_cannot_restore_it() {
    let connection = db();
    run(
        &connection,
        &anonymous(),
        Operation::SignIn {
            record: record("a", S1, "one"),
        },
    )
    .unwrap();
    let (answer, wrote) = run(
        &connection,
        &anonymous(),
        Operation::SignOut { session: S2.into() },
    )
    .unwrap();
    assert_eq!((answer["removed"].as_bool(), wrote), (Some(false), false));
    assert!(held(&connection, &anonymous()).is_some());
    let (answer, wrote) = run(
        &connection,
        &alice(),
        Operation::SignOut { session: S1.into() },
    )
    .unwrap();
    assert_eq!((answer["removed"].as_bool(), wrote), (Some(true), true));
    assert_eq!(held(&connection, &anonymous()), None);
    let refused = run(
        &connection,
        &anonymous(),
        Operation::Refresh {
            record: record("a", S1, "late"),
        },
    )
    .unwrap_err();
    assert!(refused.starts_with(ENDED));
    assert_eq!(held(&connection, &anonymous()), None);
}

#[test]
fn a_redirect_is_taken_once_and_only_for_its_state() {
    let connection = db();
    let (answer, _) = run(
        &connection,
        &anonymous(),
        Operation::TakeRedirect {
            state: STATE.into(),
        },
    )
    .unwrap();
    assert_eq!(answer["found"], "none");
    run(
        &connection,
        &anonymous(),
        Operation::BeginRedirect {
            redirect: redirect(STATE),
        },
    )
    .unwrap();
    let (answer, wrote) = run(
        &connection,
        &anonymous(),
        Operation::TakeRedirect {
            state: NONCE.into(),
        },
    )
    .unwrap();
    assert_eq!((answer["found"].as_str(), wrote), (Some("mismatch"), false));
    let (answer, wrote) = run(
        &connection,
        &alice(),
        Operation::TakeRedirect {
            state: STATE.into(),
        },
    )
    .unwrap();
    assert_eq!((answer["found"].as_str(), wrote), (Some("taken"), true));
    assert_eq!(
        serde_json::from_value::<Redirect>(answer["redirect"].clone()).unwrap(),
        redirect(STATE)
    );
    let (answer, _) = run(
        &connection,
        &anonymous(),
        Operation::TakeRedirect {
            state: STATE.into(),
        },
    )
    .unwrap();
    assert_eq!(answer["found"], "none", "a redirect answers once");
    // A newer redirect replaces an older one.
    run(
        &connection,
        &anonymous(),
        Operation::BeginRedirect {
            redirect: redirect(STATE),
        },
    )
    .unwrap();
    run(
        &connection,
        &anonymous(),
        Operation::BeginRedirect {
            redirect: redirect(NONCE),
        },
    )
    .unwrap();
    let (answer, _) = run(
        &connection,
        &anonymous(),
        Operation::TakeRedirect {
            state: STATE.into(),
        },
    )
    .unwrap();
    assert_eq!(answer["found"], "mismatch");
}

#[test]
fn malformed_operations_are_refused_and_leave_nothing_behind() {
    let mut connection = db();
    let mut broken = record("a", S1, "one");
    broken.session = "not-a-session".into();
    let identity = anonymous();
    let call = |operation: Operation| Call {
        request: Request::AuthSession { operation },
        expect_owner: None,
    };
    let refused = durable_store::execute(
        &mut connection,
        Some(&identity),
        call(Operation::SignIn { record: broken }),
        None,
    )
    .unwrap_err();
    assert!(refused.starts_with(INVALID), "{refused}");
    for operation in [
        Operation::SignOut {
            session: "x".into(),
        },
        Operation::TakeRedirect { state: "x".into() },
        Operation::BeginRedirect {
            redirect: Redirect {
                state: "x".into(),
                ..redirect(STATE)
            },
        },
    ] {
        assert!(
            durable_store::execute(&mut connection, Some(&identity), call(operation), None)
                .unwrap_err()
                .starts_with(INVALID)
        );
    }
    let bytes = durable_store::execute(
        &mut connection,
        Some(&identity),
        call(Operation::Read),
        Some(vec![1]),
    );
    assert!(bytes.unwrap_err().starts_with(INVALID));
    // Nothing was written, not even the schema step.
    let tables: i64 = connection
        .query_row(
            "SELECT count(*) FROM sqlite_master WHERE name='auth_session'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(tables, 0);
    assert!(serde_json::from_value::<Operation>(
        json!({"action": "sign_out", "session": S1, "extra": 1})
    )
    .is_err());
    assert!(serde_json::from_value::<Operation>(json!({"action": "forget_everything"})).is_err());
}

#[test]
fn writes_advance_the_store_generation_and_core_never_serves_it() {
    let mut connection = db();
    let identity = anonymous();
    let call = |operation: Operation| Call {
        request: Request::AuthSession { operation },
        expect_owner: None,
    };
    let read = durable_store::execute(
        &mut connection,
        Some(&identity),
        call(Operation::Read),
        None,
    )
    .unwrap();
    let written = durable_store::execute(
        &mut connection,
        Some(&identity),
        call(Operation::SignIn {
            record: record("a", S1, "one"),
        }),
        None,
    )
    .unwrap();
    assert_eq!((read.generation, written.generation), (Some(0), Some(1)));
    let core = Request::AuthSession {
        operation: Operation::Read,
    }
    .core_homed()
    .unwrap_err();
    assert!(core.starts_with(NOT_CORE), "{core}");
    assert_eq!(
        Request::AuthSession {
            operation: Operation::Read
        }
        .store(),
        STORE
    );
}
