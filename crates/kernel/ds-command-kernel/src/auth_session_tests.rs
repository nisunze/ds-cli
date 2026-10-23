use super::*;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use serde_json::{json, Value};

const PROJECT: &str = "ds-test-project";
const NOW: u64 = 1_800_000_000;
const S1: &str = "0123456789abcdef0123456789abcdef";
const S2: &str = "fedcba9876543210fedcba9876543210";
const A: &str = "uid-account-a";
const B: &str = "uid-account-b";

fn email(uid: &str) -> String {
    format!("{uid}@example.test")
}

fn jwt(claims: Value) -> String {
    format!(
        "{}.{}.signature",
        URL_SAFE_NO_PAD.encode(br#"{"alg":"RS256"}"#),
        URL_SAFE_NO_PAD.encode(claims.to_string())
    )
}

/// A Firebase ID token for `uid` in `project`; `mark` makes each one distinct.
fn id_token(uid: &str, project: &str, exp: u64, mark: &str) -> String {
    jwt(json!({
        "sub": uid, "email": email(uid), "aud": project,
        "iss": format!("https://securetoken.google.com/{project}"), "exp": exp, "mark": mark,
    }))
}

/// A validated login, exactly as a sign-in or a refresh produces one.
fn login(uid: &str, now: u64, mark: &str) -> crate::ValidatedLogin {
    let body = json!({
        "localId": uid, "email": email(uid), "displayName": if mark == "first" { "Operator" } else { "" },
        "idToken": id_token(uid, PROJECT, now + 3_600, mark),
        "refreshToken": format!("refresh-{uid}-{mark}"), "expiresIn": "3600",
    });
    crate::validate_firebase_login_response(
        body.to_string().as_bytes(),
        Some(&email(uid)),
        PROJECT,
        now,
    )
    .unwrap()
}

fn record(uid: &str, session: &str, now: u64) -> Record {
    Record::from_login(&login(uid, now, "first"), session, now)
}

fn signed_in(uid: &str, now: u64) -> Session {
    let mut session = Session::new();
    session.sign_in(record(uid, S1, now));
    session
}

fn refresh_call(session: &Session, now: u64) -> RefreshCall {
    match session.plan(now, false) {
        Plan::Refresh(call) => call,
        other => panic!("expected a refresh, planned {other:?}"),
    }
}

#[test]
fn a_fresh_token_is_used_and_a_due_one_refreshes_first() {
    let session = signed_in(A, NOW);
    let Plan::Use(credentials) = session.plan(NOW + 60, false) else {
        panic!("a new token is fresh");
    };
    assert_eq!(
        (credentials.uid.as_str(), credentials.email.as_str()),
        (A, email(A).as_str())
    );
    // Fifty minutes old: refreshed before it is sent.
    let call = refresh_call(&session, NOW + REFRESH_AFTER_SECONDS);
    assert_eq!(
        (call.uid.as_str(), call.refresh_token.as_str()),
        (A, "refresh-uid-account-a-first")
    );
    assert_eq!(call.generation, session.generation());
    // Inside the expiry margin, however recently it was obtained.
    let mut near = signed_in(A, NOW);
    let mut held = record(A, S1, NOW);
    held.expires_at_epoch_seconds = NOW + 30;
    near.sign_in(held);
    assert!(matches!(near.plan(NOW + 1, false), Plan::Refresh(_)));
    // A request refused as unauthenticated refreshes a fresh token too.
    assert!(matches!(session.plan(NOW + 60, true), Plan::Refresh(_)));
    assert_eq!(Session::new().plan(NOW, false), Plan::SignedOut);
}

#[test]
fn a_refresh_renews_the_same_session_and_keeps_its_generation_and_account() {
    let mut session = signed_in(A, NOW);
    let generation = session.generation();
    let call = refresh_call(&session, NOW + REFRESH_AFTER_SECONDS);
    let later = NOW + REFRESH_AFTER_SECONDS;
    let (answer, persist) = session.refreshed(
        call.generation,
        Refreshed::Session(login(A, later, "second")),
        later,
    );
    let Answer::Bearer(bearer) = answer else {
        panic!("a refresh answers its bearer");
    };
    assert_eq!(bearer.token, id_token(A, PROJECT, later + 3_600, "second"));
    // Never stored yet, so the write establishes the session.
    let Some(Persist::SignIn(renewed)) = persist else {
        panic!("an unstored session is established");
    };
    assert_eq!(renewed.session, S1);
    assert_eq!(renewed.refresh_token, "refresh-uid-account-a-second");
    assert_eq!(renewed.user.display_name.as_deref(), Some("Operator"));
    assert_eq!(renewed.obtained_at_epoch_seconds, later);
    assert_eq!(session.generation(), generation);
    assert!(matches!(session.plan(later + 1, false), Plan::Use(_)));
    // Once the store holds it, a refresh replaces it conditionally.
    session.stored(generation);
    let call = refresh_call(&session, later + REFRESH_AFTER_SECONDS);
    let (_, persist) = session.refreshed(
        call.generation,
        Refreshed::Session(login(A, later, "third")),
        later,
    );
    assert!(matches!(persist, Some(Persist::Refresh(record)) if record.session == S1));
}

#[test]
fn offline_and_unavailable_keep_the_session_and_refuse_only_an_expired_token() {
    let expired = NOW + 3_600 + 1;
    for (outcome, code) in [
        (Refreshed::Offline, OFFLINE),
        (Refreshed::Unavailable, UNAVAILABLE),
    ] {
        let mut session = signed_in(A, NOW);
        let call = refresh_call(&session, expired);
        let (answer, persist) = session.refreshed(call.generation, outcome, expired);
        let Answer::Refused(refusal) = answer else {
            panic!("an expired token is never sent");
        };
        assert_eq!(refusal.code, code);
        assert!(refusal.transient && refusal.retryable && !refusal.signed_out);
        assert_eq!(persist, None);
        assert_eq!(
            session.uid(),
            Some(A),
            "the session stays for the next attempt"
        );
    }
    // A due but unexpired token is still the session's own valid bearer.
    let due = NOW + REFRESH_AFTER_SECONDS;
    for outcome in [Refreshed::Offline, Refreshed::Unavailable] {
        let mut session = signed_in(A, NOW);
        let call = refresh_call(&session, due);
        let (answer, _) = session.refreshed(call.generation, outcome, due);
        assert!(matches!(answer, Answer::Bearer(bearer) if bearer.uid == A));
    }
}

#[test]
fn a_revoked_refresh_ends_the_session_with_a_typed_inert_refusal() {
    for outcome in [Refreshed::Ended, Refreshed::Session(login(B, NOW, "first"))] {
        let mut session = signed_in(A, NOW);
        let call = refresh_call(&session, NOW + REFRESH_AFTER_SECONDS);
        let (answer, persist) =
            session.refreshed(call.generation, outcome, NOW + REFRESH_AFTER_SECONDS);
        let Answer::Refused(refusal) = answer else {
            panic!("a revoked session refuses");
        };
        assert_eq!(refusal.code, REVOKED);
        assert!(refusal.signed_out && !refusal.transient);
        assert_eq!(persist, Some(Persist::SignOut(S1.into())));
        assert_eq!(
            session.view(),
            AuthSessionView::SignedOut {
                ended: Some(AuthEnded::Revoked)
            }
        );
        assert_eq!(session.plan(NOW, false), Plan::SignedOut);
    }
}

#[test]
fn a_refresh_for_an_older_session_never_touches_a_later_one() {
    // A delayed refresh cannot replace a later account's sign-in …
    let mut session = signed_in(A, NOW);
    let call = refresh_call(&session, NOW + REFRESH_AFTER_SECONDS);
    session.sign_in(record(B, S2, NOW));
    let (answer, persist) = session.refreshed(
        call.generation,
        Refreshed::Session(login(A, NOW, "late")),
        NOW,
    );
    assert_eq!((answer, persist), (Answer::Superseded, None));
    assert_eq!(session.uid(), Some(B));
    // … and an ended old refresh cannot sign the later account out.
    let (answer, persist) = session.refreshed(call.generation, Refreshed::Ended, NOW);
    assert_eq!((answer, persist), (Answer::Superseded, None));
    assert_eq!(session.uid(), Some(B));
    // Nor can one that started before a sign-out end the signed-out state differently.
    let mut session = signed_in(A, NOW);
    let call = refresh_call(&session, NOW + REFRESH_AFTER_SECONDS);
    session.sign_out();
    let (answer, _) = session.refreshed(call.generation, Refreshed::Ended, NOW);
    assert_eq!(answer, Answer::Superseded);
    assert_eq!(session.view(), AuthSessionView::SignedOut { ended: None });
}

#[test]
fn sign_out_hands_the_ended_record_back_once() {
    let mut session = signed_in(A, NOW);
    let generation = session.generation();
    let ended = session.sign_out().expect("the signed-in record");
    assert_eq!((ended.user.uid.as_str(), ended.session.as_str()), (A, S1));
    assert!(session.generation() > generation);
    assert_eq!(session.view(), AuthSessionView::SignedOut { ended: None });
    assert!(session.sign_out().is_none());
    assert_eq!(session.plan(NOW, true), Plan::SignedOut);
}

#[test]
fn the_held_record_is_adopted_only_while_the_page_is_restoring() {
    let mut session = Session::new();
    assert_eq!(session.view(), AuthSessionView::Restoring);
    session.restored(Some(record(A, S1, NOW)), true);
    assert_eq!(session.uid(), Some(A));
    // A sign-in decided first: the late restore changes nothing.
    let mut session = Session::new();
    session.sign_in(record(B, S2, NOW));
    session.restored(Some(record(A, S1, NOW)), true);
    assert_eq!(session.uid(), Some(B));
    let mut session = Session::new();
    session.restored(None, false);
    assert_eq!(session.view(), AuthSessionView::SignedOut { ended: None });
}

#[test]
fn a_store_that_lost_the_session_signs_this_page_out() {
    let mut session = signed_in(A, NOW);
    assert!(!session.ended_elsewhere(session.generation() + 1));
    assert_eq!(session.uid(), Some(A));
    assert!(session.ended_elsewhere(session.generation()));
    assert_eq!(session.view(), AuthSessionView::SignedOut { ended: None });
    assert!(!session.ended_elsewhere(session.generation()));
}

#[test]
fn records_admit_only_their_own_account_and_project() {
    let held = record(A, S1, NOW);
    assert_eq!(held.admits(PROJECT), Ok(()));
    assert!(held.admits("another-project").is_err());
    let mut other = record(A, S1, NOW);
    other.id_token = id_token(B, PROJECT, NOW + 3_600, "first");
    assert!(other.admits(PROJECT).is_err());
    // An expired token still admits: it refreshes.
    let mut expired = record(A, S1, NOW);
    expired.id_token = id_token(A, PROJECT, NOW - 10, "old");
    assert_eq!(expired.admits(PROJECT), Ok(()));
    let changed = |change: fn(&mut Record)| {
        let mut held = record(A, S1, NOW);
        change(&mut held);
        held
    };
    for broken in [
        changed(|held| held.session = "not-hex".into()),
        changed(|held| held.schema = "ds.auth.session/v0".into()),
        changed(|held| held.refresh_token = String::new()),
        changed(|held| held.refresh_token = "a\nb".into()),
        changed(|held| held.id_token = "x".repeat(16 * 1024 + 1)),
    ] {
        assert!(broken.validate().unwrap_err().starts_with(INVALID));
    }
    let mut spaced = record(A, S1, NOW);
    spaced.user.uid = " uid ".into();
    assert!(spaced.validate().is_err());
    // Firebase ids are opaque: case, inner spaces and slashes are the id.
    let opaque = record("Case-Sensitive UID/01", S1, NOW);
    assert_eq!(opaque.admits(PROJECT), Ok(()));
    assert_eq!(opaque.user.uid, "Case-Sensitive UID/01");
}

#[test]
fn the_view_and_the_debug_forms_never_carry_a_credential() {
    let session = signed_in(A, NOW);
    let held = record(A, S1, NOW);
    let view = serde_json::to_string(&session.view()).unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&view).unwrap(),
        json!({"state": "signed_in", "user": {"uid": A, "email": email(A), "displayName": "Operator"}})
    );
    let debug = format!(
        "{held:?} {:?} {:?}",
        session.plan(NOW, false),
        session.plan(NOW, true)
    );
    for secret in [view.as_str(), debug.as_str()] {
        assert!(!secret.contains(&held.id_token) && !secret.contains(&held.refresh_token));
    }
    assert_eq!(
        serde_json::to_value(AuthSessionView::Restoring).unwrap(),
        json!({"state": "restoring"})
    );
    assert_eq!(
        serde_json::to_value(AuthSessionView::SignedOut {
            ended: Some(AuthEnded::Revoked)
        })
        .unwrap(),
        json!({"state": "signed_out", "ended": "revoked"})
    );
}

/// The record the retired TypeScript store wrote.
fn legacy(uid: &str, token: &str, extra: Value) -> String {
    let mut value = json!({
        "user": {"uid": uid, "email": email(uid), "displayName": "Legacy Operator"},
        "token": token, "refreshToken": format!("legacy-refresh-{uid}"),
        "tokenTimestamp": (NOW - 600) as f64 * 1000.0, "loading": false,
    });
    value
        .as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    value.to_string()
}

#[test]
fn a_legacy_record_migrates_with_its_account_and_expiry() {
    let token = id_token(A, PROJECT, NOW + 3_000, "legacy");
    let with_expiry = legacy(A, &token, json!({"expiresAtEpochSeconds": NOW + 2_000}));
    let migrated = from_legacy(&with_expiry, PROJECT, S1, NOW).unwrap();
    assert_eq!(migrated.user.uid, A);
    assert_eq!(
        migrated.user.display_name.as_deref(),
        Some("Legacy Operator")
    );
    assert_eq!(migrated.session, S1);
    assert_eq!(migrated.id_token, token);
    assert_eq!(migrated.refresh_token, "legacy-refresh-uid-account-a");
    assert_eq!(migrated.expires_at_epoch_seconds, NOW + 2_000);
    assert_eq!(migrated.obtained_at_epoch_seconds, NOW - 600);
    assert_eq!(migrated.admits(PROJECT), Ok(()));
    // Older records named no expiry: the token's own claim decides.
    let without = from_legacy(&legacy(A, &token, json!({})), PROJECT, S1, NOW).unwrap();
    assert_eq!(without.expires_at_epoch_seconds, NOW + 3_000);
    // An unknown age refreshes before first use.
    let undated = from_legacy(
        &legacy(A, &token, json!({"tokenTimestamp": 0})),
        PROJECT,
        S1,
        NOW,
    )
    .unwrap();
    assert!(matches!(
        {
            let mut session = Session::new();
            session.restored(Some(undated), true);
            session.plan(NOW, false)
        },
        Plan::Refresh(_)
    ));
}

#[test]
fn a_legacy_record_of_another_account_or_project_never_migrates() {
    let ours = id_token(A, PROJECT, NOW + 3_000, "legacy");
    for text in [
        legacy(A, &id_token(B, PROJECT, NOW + 3_000, "x"), json!({})),
        legacy(
            A,
            &id_token(A, "another-project", NOW + 3_000, "x"),
            json!({}),
        ),
        legacy(A, &ours, json!({"refreshToken": null})),
        legacy(A, &ours, json!({"user": null})),
        legacy(A, "not-a-jwt", json!({})),
        "{not json".to_owned(),
        format!("{{\"pad\":\"{}\"}}", "x".repeat(MAX_LEGACY_BYTES)),
    ] {
        assert!(from_legacy(&text, PROJECT, S1, NOW)
            .unwrap_err()
            .starts_with(INVALID));
    }
}

#[test]
fn restore_prefers_the_store_and_migrates_the_legacy_record_once() {
    let token = id_token(A, PROJECT, NOW + 3_000, "legacy");
    let old = legacy(A, &token, json!({}));
    // The Kernel's record decides; the legacy one goes.
    let plan = restore(Some(record(B, S2, NOW)), Some(&old), Some(PROJECT), S1, NOW);
    assert_eq!(plan.adopt.as_ref().map(|r| r.user.uid.as_str()), Some(B));
    assert_eq!(
        (plan.write, plan.forget, plan.legacy),
        (None, None, LegacyFate::Remove)
    );
    // No Kernel record: the legacy one is written, adopted, then removed.
    let plan = restore(None, Some(&old), Some(PROJECT), S1, NOW);
    assert_eq!(plan.adopt, plan.write);
    assert_eq!(
        plan.write
            .as_ref()
            .map(|r| (r.user.uid.as_str(), r.session.as_str())),
        Some((A, S1))
    );
    assert_eq!(plan.legacy, LegacyFate::RemoveAfterWrite);
    // An unusable legacy record is removed and nobody is signed in.
    let plan = restore(None, Some("{broken"), Some(PROJECT), S1, NOW);
    assert_eq!(
        (plan.adopt, plan.write, plan.legacy),
        (None, None, LegacyFate::Remove)
    );
    // A stored record that no longer admits is forgotten.
    let mut foreign = record(A, S2, NOW);
    foreign.id_token = id_token(A, "another-project", NOW + 3_000, "x");
    let plan = restore(Some(foreign), None, Some(PROJECT), S1, NOW);
    assert_eq!(
        (plan.adopt, plan.forget, plan.legacy),
        (None, Some(S2.into()), LegacyFate::Keep)
    );
    // Nothing held; and a build without a Firebase project judges nothing.
    assert_eq!(
        restore(None, None, Some(PROJECT), S1, NOW).legacy,
        LegacyFate::Keep
    );
    let plan = restore(Some(record(A, S1, NOW)), Some(&old), None, S1, NOW);
    assert_eq!(
        (plan.adopt, plan.write, plan.legacy),
        (None, None, LegacyFate::Keep)
    );
    assert_eq!(legacy_key(PROJECT), "ds_auth:ds-test-project");
}
