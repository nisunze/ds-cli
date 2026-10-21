use super::*;

fn error(message: &str) -> Vec<u8> {
    json!({"error": {"code": 400, "message": message}})
        .to_string()
        .into_bytes()
}

#[test]
fn a_refusal_names_the_bare_code_and_the_sentence_a_person_reads() {
    let refused = refusal(&error("EMAIL_NOT_FOUND"), "Login failed.");
    assert_eq!(refused.code, "EMAIL_NOT_FOUND");
    assert_eq!(refused.message, "No account exists for that email address.");
    let weak = refusal(
        &error("WEAK_PASSWORD : Password should be at least 6 characters"),
        "Could not create the account.",
    );
    assert_eq!(weak.code, "WEAK_PASSWORD");
    assert_eq!(
        weak.message,
        "Password is too weak. Use at least 6 characters."
    );
    let unknown = refusal(&error("QUOTA_EXCEEDED"), "Login failed.");
    assert_eq!(unknown.message, "Quota exceeded");
    let unreadable = refusal(b"<html>", "Login failed.");
    assert_eq!(unreadable.message, "Login failed.");
}

#[test]
fn only_credential_shaped_refusals_ask_for_the_accounts_methods() {
    let sign_in = Call::SignIn {
        email: "a@b.rw",
        password: "x",
    };
    assert!(sign_in.asks_for_methods("INVALID_LOGIN_CREDENTIALS"));
    assert!(!sign_in.asks_for_methods("USER_DISABLED"));
    let sign_up = Call::SignUp {
        email: "a@b.rw",
        password: "x",
    };
    assert!(sign_up.asks_for_methods("EMAIL_EXISTS"));
    assert!(!sign_up.asks_for_methods("EMAIL_NOT_FOUND"));
}

#[test]
fn sign_in_methods_read_an_answer_or_say_nothing() {
    let google = json!({"registered": true, "signinMethods": ["google.com"]}).to_string();
    assert_eq!(
        sign_in_methods(200, google.as_bytes()),
        Some(vec!["google.com".to_owned()])
    );
    let absent = json!({"registered": false}).to_string();
    assert_eq!(sign_in_methods(200, absent.as_bytes()), Some(Vec::new()));
    assert_eq!(sign_in_methods(400, google.as_bytes()), None);
    assert_eq!(sign_in_methods(200, json!({}).to_string().as_bytes()), None);
}

#[test]
fn calls_carry_the_bodies_firebase_reads() {
    let google = Call::Google {
        id_token: "tok",
        request_uri: "https://app.example",
    };
    assert_eq!(
        google.endpoint(),
        format!("{IDENTITY_TOOLKIT}:signInWithIdp")
    );
    assert_eq!(
        google.body()["postBody"],
        "id_token=tok&providerId=google.com"
    );
    let reset = Call::PasswordReset { email: "a@b.rw" };
    assert_eq!(
        reset.body(),
        json!({"requestType": "PASSWORD_RESET", "email": "a@b.rw"})
    );
    assert_eq!(
        refresh_body("r t&x"),
        "grant_type=refresh_token&refresh_token=r+t%26x"
    );
    let opaque = Call::Google {
        id_token: "tok&providerId=other+id",
        request_uri: "https://app.example",
    };
    assert_eq!(
        opaque.body()["postBody"],
        "id_token=tok%26providerId%3Dother%2Bid&providerId=google.com"
    );
}

#[test]
fn replies_are_bounded_and_free_text_errors_never_reflect_credentials() {
    let refused = refusal(
        &error("QUOTA_EXCEEDED : secret@example.test?token=private"),
        "Login failed.",
    );
    assert_eq!(refused.code, "QUOTA_EXCEEDED");
    assert_eq!(refused.message, "Quota exceeded");
    let reflected = refusal(&error("secret@example.test"), "Login failed.");
    assert_eq!(reflected.code, "rejected");
    assert_eq!(reflected.message, "Login failed.");
    let oversized = vec![b'x'; crate::MAX_LOGIN_RESPONSE_BYTES + 1];
    assert_eq!(sign_in_methods(200, &oversized), None);
    assert_eq!(
        refusal(&oversized, "Login failed.").message,
        "Login failed."
    );
    assert!(matches!(
        refreshed(200, &oversized, "uid", "a@b.rw", "project-id", 0),
        Refreshed::Unavailable
    ));
    assert_eq!(
        refusal(&error("auth/email-not-found"), "Login failed.").code,
        "EMAIL_NOT_FOUND"
    );
    let oversized_input = "x".repeat(crate::MAX_LOGIN_RESPONSE_BYTES + 1);
    assert_eq!(
        Call::SignIn {
            email: "a@b.rw",
            password: &oversized_input
        }
        .validate()
        .unwrap_err()
        .code,
        "auth_input_too_large"
    );
}

#[test]
fn a_refresh_ends_only_for_a_revoked_session_and_keeps_it_through_outages() {
    let ends = |message: &str| {
        matches!(
            refreshed(400, &error(message), "uid", "a@b.rw", "project-id", 0),
            Refreshed::Ended
        )
    };
    assert!(ends("TOKEN_EXPIRED"));
    assert!(ends("INVALID_REFRESH_TOKEN"));
    assert!(ends("USER_DISABLED"));
    assert!(!ends("PROJECT_NOT_FOUND"));
    assert!(matches!(
        refreshed(503, b"", "uid", "a@b.rw", "project-id", 0),
        Refreshed::Unavailable
    ));
    assert!(matches!(
        refreshed(200, b"not json", "uid", "a@b.rw", "project-id", 0),
        Refreshed::Unavailable
    ));
    // An answer the validator refuses ends the session rather than keep a
    // token nobody vouched for.
    let malformed =
        json!({"user_id": "uid", "id_token": "x", "refresh_token": "y", "expires_in": "3600"});
    assert!(matches!(
        refreshed(
            200,
            malformed.to_string().as_bytes(),
            "uid",
            "a@b.rw",
            "project-id",
            0
        ),
        Refreshed::Ended
    ));
}

#[test]
fn sign_in_and_refresh_use_the_real_kernel_identity_audience_and_expiry_validator() {
    use base64::Engine;
    let claims = json!({"sub":"uid-a", "email":"operator@example.test", "aud":"project-id",
        "iss":"https://securetoken.google.com/project-id", "exp":4600});
    let token = format!(
        "header.{}.signature",
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(claims.to_string())
    );
    let answer = json!({"localId":"uid-a", "email":"operator@example.test", "idToken":token,
        "refreshToken":"refresh-token", "expiresIn":"3600"});
    let login = session(
        answer.to_string().as_bytes(),
        Some("operator@example.test"),
        "project-id",
        1000,
    )
    .unwrap();
    assert_eq!(login.principal().uid(), "uid-a");
    assert!(session(
        answer.to_string().as_bytes(),
        Some("another@example.test"),
        "project-id",
        1000
    )
    .is_err());
    assert!(session(
        answer.to_string().as_bytes(),
        Some("operator@example.test"),
        "another-project",
        1000
    )
    .is_err());
    assert!(session(
        answer.to_string().as_bytes(),
        Some("operator@example.test"),
        "project-id",
        4600
    )
    .is_err());
    let refresh = json!({"user_id":"uid-a", "id_token":token, "refresh_token":"next-refresh", "expires_in":"3600"});
    assert!(matches!(
        refreshed(
            200,
            refresh.to_string().as_bytes(),
            "uid-a",
            "operator@example.test",
            "project-id",
            1000
        ),
        Refreshed::Session(_)
    ));
    assert!(matches!(
        refreshed(
            200,
            refresh.to_string().as_bytes(),
            "uid-b",
            "operator@example.test",
            "project-id",
            1000
        ),
        Refreshed::Ended
    ));
}
