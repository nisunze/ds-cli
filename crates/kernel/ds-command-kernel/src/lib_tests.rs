use super::*;

const NOW: u64 = 1_700_000_000;
const PROJECT: &str = "ds-testing";
const UID: &str = "Case-Sensitive UID/01";
const EMAIL: &str = "operator@example.com";

fn token(uid: &str, email: &str, audience: &str, issuer: &str, exp: u64) -> String {
    let header = URL_SAFE_NO_PAD.encode(br#"{"alg":"RS256","typ":"JWT"}"#);
    let claims = URL_SAFE_NO_PAD.encode(
        serde_json::to_vec(&serde_json::json!({
            "sub": uid,
            "email": email,
            "aud": audience,
            "iss": issuer,
            "exp": exp,
        }))
        .unwrap(),
    );
    format!("{header}.{claims}.signature")
}

fn response(overrides: Value) -> Vec<u8> {
    let mut value = serde_json::json!({
        "localId": UID,
        "email": EMAIL,
        "idToken": token(
            UID,
            EMAIL,
            PROJECT,
            &format!("https://securetoken.google.com/{PROJECT}"),
            NOW + 3_600,
        ),
        "refreshToken": "protected-refresh",
        "expiresIn": "3600",
        "providerUserInfo": [{
            "displayName": "DS Operator",
            "photoUrl": "https://images.example/operator.png"
        }]
    });
    if let (Some(target), Some(source)) = (value.as_object_mut(), overrides.as_object()) {
        target.extend(source.clone());
    }
    serde_json::to_vec(&value).unwrap()
}

#[test]
fn validates_one_bounded_login_for_native_and_browser_projection() {
    let login = validate_firebase_login_response(&response(Value::Null), Some(EMAIL), PROJECT, NOW)
        .unwrap();
    assert_eq!(login.principal().uid(), UID);
    assert_eq!(login.principal().display_name(), Some("DS Operator"));
    assert_eq!(login.expires_at_epoch_seconds(), NOW + 3_600);
    let browser: Value = serde_json::from_str(&login.to_browser_json().unwrap()).unwrap();
    assert_eq!(browser["schema"], LOGIN_CONTRACT_SCHEMA);
    assert_eq!(browser["user"]["uid"], UID);
    assert_eq!(browser["refreshToken"], "protected-refresh");
}

#[test]
fn fails_closed_on_identity_audience_issuer_and_expiry_mismatch() {
    let cases = [
        token(
            "another-uid",
            EMAIL,
            PROJECT,
            &format!("https://securetoken.google.com/{PROJECT}"),
            NOW + 3_600,
        ),
        token(
            UID,
            "other@example.com",
            PROJECT,
            &format!("https://securetoken.google.com/{PROJECT}"),
            NOW + 3_600,
        ),
        token(
            UID,
            EMAIL,
            "other-project",
            &format!("https://securetoken.google.com/{PROJECT}"),
            NOW + 3_600,
        ),
        token(
            UID,
            EMAIL,
            PROJECT,
            "https://attacker.example/ds-testing",
            NOW + 3_600,
        ),
        token(
            UID,
            EMAIL,
            PROJECT,
            &format!("https://securetoken.google.com/{PROJECT}"),
            NOW,
        ),
    ];
    for id_token in cases {
        let error = validate_firebase_login_response(
            &response(serde_json::json!({ "idToken": id_token })),
            Some(EMAIL),
            PROJECT,
            NOW,
        )
        .unwrap_err();
        assert_eq!(error.kind(), LoginErrorKind::IdentityMismatch);
    }
}

#[test]
fn rejects_unbounded_or_malformed_login_envelopes() {
    assert_eq!(
        validate_firebase_login_response(
            &vec![b'x'; MAX_LOGIN_RESPONSE_BYTES + 1],
            Some(EMAIL),
            PROJECT,
            NOW,
        )
        .unwrap_err()
        .kind(),
        LoginErrorKind::UnreadableResponse,
    );
    assert_eq!(
        validate_firebase_login_response(b"{}", Some(EMAIL), PROJECT, NOW)
            .unwrap_err()
            .kind(),
        LoginErrorKind::UnreadableResponse,
    );
    assert_eq!(
        validate_firebase_login_response(
            &response(serde_json::json!({ "userId": "another-uid" })),
            Some(EMAIL),
            PROJECT,
            NOW,
        )
        .unwrap_err()
        .kind(),
        LoginErrorKind::IdentityMismatch,
    );
}

#[test]
fn classifies_failed_password_exchange_without_leaking_response_text() {
    let response = |message: &str| {
        serde_json::to_vec(&serde_json::json!({ "error": { "message": message } })).unwrap()
    };
    assert_eq!(
        classify_firebase_auth_failure(400, &response("INVALID_LOGIN_CREDENTIALS")),
        FirebaseAuthFailureKind::InvalidCredentials,
    );
    assert_eq!(
        classify_firebase_auth_failure(400, &response("OPERATION_NOT_ALLOWED")),
        FirebaseAuthFailureKind::PasswordSignInUnavailable,
    );
    assert_eq!(
        classify_firebase_auth_failure(400, &response("USER_DISABLED")),
        FirebaseAuthFailureKind::AccountDisabled,
    );
    assert_eq!(
        classify_firebase_auth_failure(429, b"not exposed"),
        FirebaseAuthFailureKind::Transient,
    );
    assert_eq!(
        classify_firebase_auth_failure(400, b"not-json"),
        FirebaseAuthFailureKind::UnreadableResponse,
    );
}

#[test]
fn native_portfolio_read_uses_the_solar_contract_projection() {
    let document = portfolio_v2_fixture();
    let result = validate_solar_portfolio_result(&document, "run-1").unwrap();
    let result: Value = serde_json::from_str(&result).unwrap();
    assert_eq!(result["trace"]["city_ids"], serde_json::json!(["a", "b"]));
    assert_eq!(result["trace"]["representative_city"], "a");
}

fn portfolio_v2_fixture() -> Vec<u8> {
    let mut sections = ds_solar_contracts::portfolio::PORTFOLIO_RESULT_SECTIONS
        .iter()
        .map(|name| ((*name).to_string(), serde_json::json!({ "value": 1 })))
        .collect::<serde_json::Map<_, _>>();
    sections.insert(
        "representative_graphs".to_string(),
        serde_json::json!({ "_strategy": "first" }),
    );
    let city = |city_id: &str, selected_system: &str, customers: i64, energy: f64| {
        serde_json::json!({
            "city_id": city_id,
            "display_name": city_id.to_ascii_uppercase(),
            "city_run_id": format!("run-{city_id}"),
            "input_digest": format!("sha256:{}", "d".repeat(64)),
            "result_digest": format!("sha256:{}", "e".repeat(64)),
            "contribution_digest": format!("sha256:{}", "f".repeat(64)),
            "calculation_projection_digest": format!("sha256:{}", "1".repeat(64)),
            "selected_system": selected_system,
            "inverter_count": 1,
            "pv_capacity_mw": 1.0,
            "battery_capacity_mwh": 1.0,
            "generator_count": 1.0,
            "diesel_capacity_mw": 1.0,
            "total_customers": customers,
            "annual_energy_kwh": energy,
        })
    };
    serde_json::to_vec(&serde_json::json!({
        "schema_version": ds_solar_contracts::PORTFOLIO_SCHEMA_VERSION_V2,
        "engine": { "name": "ds-solar", "version": "0.1.0" },
        "project_id": "project",
        "root": "root",
        "portfolio_id": "portfolio",
        "portfolio_name": "Portfolio",
        "membership_revision": format!("sha256:{}", "a".repeat(64)),
        "run_id": "run-1",
        "input_digest": format!("sha256:{}", "b".repeat(64)),
        "assumptions": { "currency": "XAF", "project_years": 25, "discount_rate": 0.1 },
        "representative_city_id": "a",
        "cities": [city("a", "hybrid", 10, 100.0), city("b", "solar_battery", 20, 200.0)],
        "city_count": 2,
        "total_customers": 30,
        "annual_energy_kwh": 300.0,
        "systems": {
            "hybrid": {
                "cities": 1, "inverter_count": 1, "pv_capacity_mw": 1.0,
                "battery_capacity_mwh": 1.0, "generator_count": 1.0,
                "diesel_capacity_mw": 1.0
            },
            "solar_battery": {
                "cities": 1, "inverter_count": 1, "pv_capacity_mw": 1.0,
                "battery_capacity_mwh": 1.0, "generator_count": 1.0,
                "diesel_capacity_mw": 1.0
            }
        },
        "sections": sections,
        "portfolio_digest": format!("sha256:{}", "c".repeat(64)),
    }))
    .unwrap()
}
