use super::*;
use serde_json::json;

const CLIENT: &str = "1234567890-abcdef.apps.googleusercontent.com";
const STATE: &str = "0123456789abcdef0123456789abcdef";
const NONCE: &str = "fedcba9876543210fedcba9876543210";
const OTHER: &str = "00112233445566778899aabbccddeeff";
const NOW: u64 = 1_800_000_000;

fn google_token(claims: Value) -> String {
    format!(
        "{}.{}.signature",
        URL_SAFE_NO_PAD.encode(br#"{"alg":"RS256"}"#),
        URL_SAFE_NO_PAD.encode(claims.to_string())
    )
}

fn issued(nonce: &str, audience: Value, issuer: &str, exp: u64) -> String {
    google_token(
        json!({"iss": issuer, "aud": audience, "sub": "google-subject", "email": "a@example.test", "nonce": nonce, "exp": exp}),
    )
}

fn pending() -> Redirect {
    begin(
        Some(CLIENT),
        "https://app.example",
        "/auth/google",
        "/projects?tab=work",
        STATE,
        NONCE,
        NOW,
    )
    .unwrap()
    .0
}

fn callback(outcome: Outcome) -> Callback {
    Callback {
        state: STATE.into(),
        outcome,
    }
}

fn token_callback(token: String) -> Callback {
    callback(Outcome::IdToken(token))
}

fn good() -> String {
    issued(
        NONCE,
        json!(CLIENT),
        "https://accounts.google.com",
        NOW + 3_600,
    )
}

#[test]
fn begin_names_the_client_the_callback_the_state_and_the_nonce() {
    let (redirect, address) = begin(
        Some(CLIENT),
        "https://app.example",
        "/grid/auth/google",
        "/projects?tab=work",
        STATE,
        NONCE,
        NOW,
    )
    .unwrap();
    let url = url::Url::parse(&address).unwrap();
    assert_eq!(
        format!(
            "{}://{}{}",
            url.scheme(),
            url.host_str().unwrap(),
            url.path()
        ),
        AUTHORIZATION_ENDPOINT
    );
    let query: std::collections::BTreeMap<String, String> =
        url.query_pairs().into_owned().collect();
    assert_eq!(query["client_id"], CLIENT);
    assert_eq!(
        query["redirect_uri"],
        "https://app.example/grid/auth/google"
    );
    assert_eq!(query["response_type"], "id_token");
    assert_eq!(query["scope"], "openid email profile");
    assert_eq!(
        (query["state"].as_str(), query["nonce"].as_str()),
        (STATE, NONCE)
    );
    assert_eq!(query["prompt"], "select_account");
    assert_eq!(
        redirect,
        Redirect {
            state: STATE.into(),
            nonce: NONCE.into(),
            redirect_uri: "https://app.example/grid/auth/google".into(),
            return_path: "/projects?tab=work".into(),
            created_at_epoch_seconds: NOW,
        }
    );
    assert_eq!(request_uri(&redirect), "https://app.example");
    // The local stack is a loopback origin.
    let local = begin(
        Some(CLIENT),
        "http://localhost:5173",
        "/auth/google",
        "",
        STATE,
        NONCE,
        NOW,
    )
    .unwrap();
    assert_eq!(local.0.redirect_uri, "http://localhost:5173/auth/google");
    assert_eq!(local.0.return_path, "/");
}

#[test]
fn begin_refuses_an_unconfigured_build_and_any_foreign_address() {
    let refused = |client: Option<&str>, origin: &str, callback: &str, returning: &str| {
        begin(client, origin, callback, returning, STATE, NONCE, NOW)
            .unwrap_err()
            .code
    };
    assert_eq!(
        refused(None, "https://app.example", "/auth/google", "/"),
        UNCONFIGURED
    );
    assert_eq!(
        refused(Some(" "), "https://app.example", "/auth/google", "/"),
        UNCONFIGURED
    );
    for origin in [
        "http://app.example",
        "https://app.example/path",
        "https://user@app.example",
        "javascript:alert(1)",
    ] {
        assert_eq!(
            refused(Some(CLIENT), origin, "/auth/google", "/"),
            INVALID,
            "{origin}"
        );
    }
    for callback in [
        "/auth/google/extra",
        "//evil.example/auth/google",
        "/x/../auth/google",
        "/auth/google?x=1",
        "auth/google",
    ] {
        assert_eq!(
            refused(Some(CLIENT), "https://app.example", callback, "/"),
            INVALID,
            "{callback}"
        );
    }
    for returning in [
        "//evil.example/",
        "https://evil.example/",
        "\\\\evil",
        "/a\nb",
    ] {
        assert_eq!(
            refused(
                Some(CLIENT),
                "https://app.example",
                "/auth/google",
                returning
            ),
            INVALID,
            "{returning}"
        );
    }
    assert_eq!(
        begin(
            Some(CLIENT),
            "https://app.example",
            "/auth/google",
            "/",
            "short",
            NONCE,
            NOW
        )
        .unwrap_err()
        .code,
        INVALID
    );
}

#[test]
fn the_callback_carries_a_token_or_googles_error_and_nothing_ambiguous() {
    let parsed = parse_callback(&format!(
        "#id_token=a.b.c&state={STATE}&authuser=0&prompt=consent"
    ))
    .unwrap();
    assert_eq!(parsed, token_callback("a.b.c".into()));
    let declined = parse_callback(&format!("error=access_denied&state={STATE}")).unwrap();
    assert_eq!(declined, callback(Outcome::Error("access_denied".into())));
    for fragment in [
        String::new(),
        "#".into(),
        format!("#id_token=a.b.c&state={STATE}&state={OTHER}"),
        format!("#id_token=a.b.c&id_token=d.e.f&state={STATE}"),
        "#id_token=a.b.c".into(),
        "#id_token=a.b.c&state=NOT-HEX".into(),
        format!("#id_token=a.b.c&error=access_denied&state={STATE}"),
        format!("#state={STATE}"),
        format!("#id_token=&state={STATE}"),
        format!("#id_token={}&state={STATE}", "a".repeat(MAX_FRAGMENT)),
    ] {
        assert_eq!(
            parse_callback(&fragment).unwrap_err().code,
            INVALID,
            "{fragment:.40}"
        );
    }
    // The debug form never shows the token.
    assert!(!format!("{parsed:?}").contains("a.b.c"));
}

#[test]
fn verify_admits_only_the_pending_redirect_with_its_nonce() {
    let taken = || Taken::Taken {
        redirect: pending(),
    };
    let (token, redirect) =
        verify(taken(), token_callback(good()), Some(CLIENT), NOW + 30).unwrap();
    assert_eq!(
        (token, redirect.return_path.as_str()),
        (good(), "/projects?tab=work")
    );
    let array = issued(
        NONCE,
        json!(["another-client", CLIENT]),
        "accounts.google.com",
        NOW + 3_600,
    );
    assert!(verify(taken(), token_callback(array), Some(CLIENT), NOW).is_ok());
    let code = |taken: Taken, callback: Callback, now: u64| {
        verify(taken, callback, Some(CLIENT), now).unwrap_err().code
    };
    assert_eq!(code(Taken::None, token_callback(good()), NOW), NOT_PENDING);
    assert_eq!(
        code(Taken::Mismatch, token_callback(good()), NOW),
        STATE_MISMATCH
    );
    let mut stray = token_callback(good());
    stray.state = OTHER.into();
    assert_eq!(code(taken(), stray, NOW), STATE_MISMATCH);
    assert_eq!(
        code(
            taken(),
            token_callback(good()),
            NOW + REDIRECT_TTL_SECONDS + 1
        ),
        EXPIRED
    );
    assert_eq!(
        code(
            taken(),
            callback(Outcome::Error("access_denied".into())),
            NOW
        ),
        CANCELLED
    );
    assert_eq!(
        code(
            taken(),
            callback(Outcome::Error("server_error".into())),
            NOW
        ),
        FAILED
    );
    let replayed = issued(
        OTHER,
        json!(CLIENT),
        "https://accounts.google.com",
        NOW + 3_600,
    );
    assert_eq!(code(taken(), token_callback(replayed), NOW), NONCE_MISMATCH);
    for token in [
        issued(
            NONCE,
            json!("another-client"),
            "https://accounts.google.com",
            NOW + 3_600,
        ),
        issued(NONCE, json!(CLIENT), "https://evil.example", NOW + 3_600),
        issued(NONCE, json!(CLIENT), "https://accounts.google.com", NOW - 1),
        "not-a-token".into(),
    ] {
        assert_eq!(code(taken(), token_callback(token), NOW), INVALID);
    }
    assert_eq!(
        verify(taken(), token_callback(good()), None, NOW)
            .unwrap_err()
            .code,
        UNCONFIGURED
    );
}

#[test]
fn a_redirect_record_admits_only_its_bounds() {
    assert!(pending().validate().is_ok());
    let mut broken = pending();
    broken.nonce = "short".into();
    assert!(broken.validate().is_err());
    let mut broken = pending();
    broken.return_path = "//evil.example".into();
    assert!(broken.validate().is_err());
    let taken: Taken = serde_json::from_value(
        json!({"found": "taken", "redirect": pending(), "generation": 3, "owner": "x"}),
    )
    .unwrap();
    assert_eq!(
        taken,
        Taken::Taken {
            redirect: pending()
        }
    );
    let none: Taken =
        serde_json::from_value(json!({"found": "none", "generation": 1, "owner": "x"})).unwrap();
    assert_eq!(none, Taken::None);
}
