//! The bounded loopback client, shared by every command that borrows the
//! paired application's authority.
//!
//! Two calls, and the shape of each is the whole security argument:
//!
//! * [`session`] reads the application's own bounded view of itself — paired,
//!   signed in, which project and active design context. It returns no
//!   credential and never has.
//! * [`invoke`] asks the application to *do* one named semantic operation.
//!   The operation runs inside the application, under the application's
//!   identity, and what comes back is its result.
//!
//! That second shape is what keeps the crate-level invariant true while still
//! letting `ds` reach authenticated work: the JWT is never fetched, never
//! passed as an argument and never held here, because the call that needs it
//! is made by the process that already has it. `ds` asks for an outcome, not
//! for a credential.

use std::time::Duration;

use ds_cli_contract::outcome::{ExitClass, Failure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::discover::{self, Discovery};

/// The pairing handshake is loopback and immediate; anything slower is a dead
/// descriptor rather than a busy application.
pub const SESSION_TIMEOUT: Duration = Duration::from_secs(5);

/// Bound on any bridge response body.
pub const MAX_RESPONSE_BYTES: u64 = 8 * 1024 * 1024;

/// Resolve the paired application, or refuse with the remedy.
pub fn paired(explicit: Option<&str>) -> Result<discover::Found, Failure> {
    match discover::discover(explicit) {
        Discovery::Paired(found) => Ok(*found),
        Discovery::None => Err(Failure::unavailable(
            "desktop_not_paired",
            "no DS GridDesign session is running on this machine",
        )
        .remedy("start DS GridDesign and sign in, then retry")
        .next("ds desktop status")),
        Discovery::Ambiguous(choices) => Err(Failure::invalid(
            "desktop_ambiguous",
            "more than one DS GridDesign session is running",
        )
        .remedy("name one with --desktop-descriptor <path>")
        .detail(json!({
            "descriptors": choices
                .iter()
                .map(|(profile, path)| json!({
                    "profile": profile,
                    "descriptor": path.display().to_string()
                }))
                .collect::<Vec<_>>()
        }))),
        Discovery::Unusable { path, reason } => Err(Failure::unavailable(
            "desktop_unreachable",
            "the bridge descriptor cannot be used",
        )
        .remedy("restart DS GridDesign and retry")
        .detail(json!({ "descriptor": path.display().to_string(), "reason": reason }))),
    }
}

/// Ask the paired application to perform one named semantic operation.
///
/// `timeout` is the caller's, because the operations differ by orders of
/// magnitude: reading a workspace is instant, downloading seven reference
/// bundles is a minute.
pub fn invoke(
    descriptor: &discover::Descriptor,
    operation: &'static str,
    arguments: Value,
    identity_fence: &IdentityFence,
    timeout: Duration,
) -> Result<Value, Failure> {
    let request = json!({
        "operation": operation,
        "arguments": arguments,
        "identity_fence": identity_fence,
    });
    let response = ureq::post(&format!("{}/v1/invoke", descriptor.url))
        .header("authorization", &format!("Bearer {}", descriptor.token))
        .config()
        .timeout_global(Some(timeout))
        // Operation refusals are protocol responses whose JSON body carries
        // the actionable application error. If ureq promotes 4xx to a
        // transport error here, the CLI mislabels a real 422 refusal as an
        // unreachable desktop and discards that body.
        .http_status_as_error(false)
        .build()
        .send_json(request);

    let mut response = match response {
        Ok(response) => response,
        Err(error) => {
            return Err(Failure::unavailable(
                "desktop_unreachable",
                format!("the paired session did not answer `{operation}`"),
            )
            .remedy("DS GridDesign may have exited; restart it and retry")
            .detail(json!({ "detail": bounded(&error.to_string()) })));
        }
    };

    let status = response.status().as_u16();
    let body = response
        .body_mut()
        .with_config()
        .limit(MAX_RESPONSE_BYTES)
        .read_to_string()
        .map_err(|_| {
            Failure::unavailable(
                "desktop_unreadable",
                format!("the reply to `{operation}` could not be read"),
            )
            .remedy("restart DS GridDesign and retry")
        })?;
    let parsed: Value = serde_json::from_str(&body).unwrap_or(Value::Null);

    if status == 422
        && let Some(failure) = structured_desktop_refusal(operation, &parsed, status)
    {
        return Err(failure);
    }

    match status {
        200 => Ok(parsed),
        401 => Err(Failure::unauthorized(
            "pairing_rejected",
            "the application refused this pairing secret",
        )
        .remedy("the descriptor is stale; restart DS GridDesign")),
        400 if parsed["error"] == "unknown_operation" => Err(Failure::unavailable(
            "desktop_operation_unsupported",
            format!("this DS GridDesign build does not offer `{operation}`"),
        )
        .remedy(
            "update DS GridDesign to a release that does; `ds desktop status` reports the profile",
        )),
        409 if desktop_error_code(&parsed) == Some("auth_context_mismatch") => Err(Failure::conflict(
            "auth_context_mismatch",
            "the paired map identity changed or does not match the checked invocation authority",
        )
        .remedy("re-run after confirming the paired account, lane, audience, and project")),
        _ => Err(Failure::failed(
            "desktop_refused",
            format!("the paired session refused `{operation}`"),
        )
        .remedy("read detail for the application's own message")
        .detail(json!({
            "http_status": status,
            "detail": operation_error_detail(operation, parsed["error"].as_str().unwrap_or(&body))
        }))),
    }
}

fn desktop_error_code(parsed: &Value) -> Option<&str> {
    parsed["error"]
        .as_str()
        .or_else(|| parsed["error"]["code"].as_str())
}

/// Preserve the application's typed refusal instead of turning every HTTP
/// 422 into `desktop_refused`. Invalid or legacy payloads deliberately fall
/// back to the old conservative classification in [`invoke`].
///
/// This used to allowlist two operations by name, on the reasoning that "a new
/// frontend code is not automatically a CLI contract". The reasoning was right
/// and the mechanism was wrong: it made *every other* operation's refusal
/// arrive as `desktop_refused` / class `failed` / no remedy, so an input error
/// and a server fault were indistinguishable and a caller was told to retry
/// something that could never succeed. The contract is now enforced where it
/// belongs — `refusal_codes_are_declared` in `ds/tests/bridge_parity.rs`
/// requires every code the application can emit to be declared in a command's
/// `Refusal` table, which `ds capabilities` publishes.
///
/// What still falls back: anything that is not a complete, well-formed
/// structured refusal. A legacy string error, an unknown class, a malformed
/// code and an empty message are all left to `desktop_refused`.
fn structured_desktop_refusal(_operation: &str, parsed: &Value, status: u16) -> Option<Failure> {
    let error = parsed["error"].as_object()?;
    let class = error.get("class")?.as_str()?;
    let code = error.get("code")?.as_str()?;
    if code.is_empty()
        || code.len() > 80
        || !code
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
    {
        return None;
    }
    let message = bounded(error.get("message")?.as_str()?);
    if message.is_empty() {
        return None;
    }
    // The class decides how a caller must react, so an unrecognized one is
    // refused rather than guessed: classing an input error as `failed` tells a
    // caller to retry work that can never succeed.
    //
    // Carried as `ExitClass` rather than the per-class `Failure::invalid` and
    // friends because the code is the application's, not a literal here. The
    // constructor helpers would read as if this file owned six codes, and
    // `refusal_coverage`'s source scan would report exactly that.
    let class = match class {
        "invalid_input" => ExitClass::InvalidInput,
        "unavailable" => ExitClass::Unavailable,
        "unauthorized" => ExitClass::Unauthorized,
        "conflict" => ExitClass::Conflict,
        "failed" => ExitClass::Failed,
        "internal" => ExitClass::Internal,
        _ => return None,
    };
    let mut failure = Failure::new(class, code, message);
    if let Some(remedy) = error.get("remedy").and_then(Value::as_str) {
        let remedy = bounded(remedy);
        if !remedy.is_empty() {
            failure = failure.remedy(remedy);
        }
    }
    Some(failure.detail(json!({ "http_status": status })))
}

/// Non-secret snapshot fence carried beside (never inside) operation inputs.
///
/// The application rechecks every field immediately before dispatch. This
/// closes the gap in which a map account or project could change after the
/// CLI's provider arbitration but before the operation executes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IdentityFence {
    pub uid: String,
    pub lane: String,
    pub credential_audience_sha256: String,
    pub project: Option<String>,
    pub session_revision: u64,
}

impl IdentityFence {
    pub fn from_session(value: &Value) -> Result<Self, Failure> {
        let fence: Self = serde_json::from_value(json!({
            "uid": value.get("uid").cloned().unwrap_or(Value::Null),
            "lane": value.get("lane").cloned().unwrap_or(Value::Null),
            "credential_audience_sha256": value
                .get("credential_audience_sha256")
                .cloned()
                .unwrap_or(Value::Null),
            "project": value.get("project").cloned().unwrap_or(Value::Null),
            "session_revision": value
                .get("session_revision")
                .cloned()
                .unwrap_or(Value::Null),
        }))
        .map_err(|_| {
            Failure::conflict(
                "auth_context_mismatch",
                "the paired map did not publish a complete signed-in identity fence",
            )
            .remedy("update DS GridDesign, sign in, select the intended project, and retry")
        })?;
        if fence.lane == "local" {
            return Err(Failure::unavailable(
                "desktop_operation_unsupported",
                "the paired local Desktop build has no provisioned native lane",
            )
            .remedy(
                "use a provisioned Canary or Stable DS GridDesign build for paired CLI operations",
            ));
        }
        if fence.uid.is_empty()
            || !matches!(fence.lane.as_str(), "stable" | "canary")
            || fence.credential_audience_sha256.len() != 64
            || !fence
                .credential_audience_sha256
                .bytes()
                .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
            || fence.session_revision == 0
        {
            return Err(Failure::conflict(
                "auth_context_mismatch",
                "the paired map published a malformed identity fence",
            )
            .remedy("update DS GridDesign, sign in, and retry"));
        }
        Ok(fence)
    }
}

/// Print failures carry a bounded per-format receipt after the headline.
/// Preserve it so a CLI caller can correct the failing template.
fn operation_error_detail(operation: &str, detail: &str) -> String {
    if operation == "printing.export" {
        detail
            .chars()
            .filter(|c| !c.is_control() || *c == '\n')
            .take(2_000)
            .collect()
    } else {
        bounded(detail)
    }
}

/// Keep an upstream error string short and free of anything path- or
/// credential-shaped before it becomes part of a result.
pub fn bounded(detail: &str) -> String {
    detail
        .lines()
        .next()
        .unwrap_or_default()
        .chars()
        .take(160)
        .collect()
}

/// The application's own bounded view: pairing, identity, project and context.
///
/// Returns no credential and never has — the fields are exactly what the
/// application already displays in its own window.
pub fn session(descriptor: &discover::Descriptor) -> Result<Value, Failure> {
    let response = ureq::get(&format!("{}/v1/session", descriptor.url))
        .header("authorization", &format!("Bearer {}", descriptor.token))
        .config()
        .timeout_global(Some(SESSION_TIMEOUT))
        .build()
        .call();

    let mut response = match response {
        Ok(response) => response,
        Err(error) => {
            return Err(Failure::unavailable(
                "desktop_unreachable",
                "the paired session did not answer",
            )
            .remedy("DS GridDesign may have exited; restart it and retry")
            .detail(json!({ "detail": bounded(&error.to_string()) })));
        }
    };
    if response.status() == 401 {
        return Err(Failure::unauthorized(
            "pairing_rejected",
            "the application refused this pairing secret",
        )
        .remedy("the descriptor is stale; restart DS GridDesign"));
    }
    if response.status() != 200 {
        return Err(Failure::unavailable(
            "desktop_refused",
            "the paired session refused the status request",
        )
        .remedy("restart DS GridDesign and retry")
        .detail(json!({ "http_status": response.status().as_u16() })));
    }
    let body = response
        .body_mut()
        .with_config()
        .limit(MAX_RESPONSE_BYTES)
        .read_to_string()
        .map_err(|_| {
            Failure::unavailable(
                "desktop_unreadable",
                "the session response could not be read",
            )
            .remedy("restart DS GridDesign and retry")
        })?;
    serde_json::from_str(&body).map_err(|_| {
        Failure::unavailable(
            "desktop_contract_mismatch",
            "the paired session's reply does not match this build's contract",
        )
        .remedy("update DS GridDesign and `ds` to matching releases")
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signed_in_local_desktop_is_unsupported_not_an_identity_mismatch() {
        let error = IdentityFence::from_session(&json!({
            "uid": "uid-1",
            "lane": "local",
            "credential_audience_sha256": "a".repeat(64),
            "project": "project-1",
            "session_revision": 7,
        }))
        .expect_err("local Desktop must not impersonate a provisioned release lane");
        assert_eq!(error.code(), "desktop_operation_unsupported");
        assert!(
            error
                .remedy_text()
                .is_some_and(|remedy| remedy.contains("provisioned Canary or Stable"))
        );
    }

    #[test]
    fn structured_desktop_refusals_keep_code_class_and_remedy() {
        let failure = structured_desktop_refusal(
            "data.admin_bounds.read",
            &json!({"error": {
                "class": "unavailable",
                "code": "admin_authority_unavailable",
                "message": "authority did not answer",
                "remedy": "retry unchanged"
            }}),
            422,
        )
        .expect("typed refusal");
        assert_eq!(failure.code(), "admin_authority_unavailable");
        assert_eq!(failure.class().token(), "unavailable");
        assert_eq!(failure.message(), "authority did not answer");
        assert_eq!(failure.remedy_text(), Some("retry unchanged"));
        assert_eq!(failure.detail_value().unwrap()["http_status"], 422);
    }

    #[test]
    fn malformed_structured_desktop_refusals_do_not_invent_codes() {
        // Shape, not membership, is what disqualifies a refusal now. An
        // unknown class is still refused rather than guessed, because the
        // class is what tells a caller whether retrying can ever work.
        for error in [
            json!({"class": "success", "code": "admin_authority_unavailable", "message": "bad class"}),
            json!({"class": "unavailable", "code": "NOT-STABLE", "message": "bad code"}),
            json!({"class": "unavailable", "code": "admin_authority_unavailable", "message": ""}),
            json!({"code": "admin_authority_unavailable", "message": "no class"}),
            json!("a legacy string error"),
        ] {
            assert!(
                structured_desktop_refusal(
                    "data.admin_bounds.read",
                    &json!({"error": error}),
                    422,
                )
                .is_none()
            );
        }
    }

    /// Any operation may now carry a typed refusal, and every class maps.
    ///
    /// This replaces an allowlist of two operations. It was there so the
    /// application could not mint a code `ds` does not document; that
    /// guarantee now lives in `every_application_refusal_code_is_documented`,
    /// which fails the build instead of silently degrading the refusal. The
    /// old behaviour is what a stress session met as `desktop_refused` for six
    /// unrelated causes, including an input error reported as class `failed`.
    #[test]
    fn any_operation_may_carry_a_typed_refusal_and_every_class_maps() {
        for (class, expected) in [
            ("invalid_input", "invalid_input"),
            ("unavailable", "unavailable"),
            ("unauthorized", "unauthorized"),
            ("conflict", "conflict"),
            ("failed", "failed"),
            ("internal", "internal"),
        ] {
            let failure = structured_desktop_refusal(
                "design.process.batch",
                &json!({"error": {
                    "class": class,
                    "code": "report_grouping_not_prepared",
                    "message": "prepare the applied grouping first",
                    "remedy": "run consumer-grouping apply while online"
                }}),
                422,
            )
            .expect("typed refusal for a non-allowlisted operation");
            assert_eq!(failure.code(), "report_grouping_not_prepared");
            assert_eq!(failure.class().token(), expected);
            assert_eq!(
                failure.remedy_text(),
                Some("run consumer-grouping apply while online")
            );
        }
    }

    /// A refusal without a remedy stays a refusal, and says nothing it cannot.
    #[test]
    fn a_missing_or_empty_remedy_is_omitted_rather_than_invented() {
        for error in [
            json!({"class": "conflict", "code": "report_grouping_not_prepared", "message": "m"}),
            json!({"class": "conflict", "code": "report_grouping_not_prepared", "message": "m", "remedy": ""}),
        ] {
            let failure =
                structured_desktop_refusal("design.process.batch", &json!({"error": error}), 422)
                    .expect("typed refusal");
            assert_eq!(failure.remedy_text(), None);
        }
    }

    #[test]
    fn auth_context_code_accepts_legacy_and_structured_desktop_shapes() {
        assert_eq!(
            desktop_error_code(&json!({"error": "auth_context_mismatch"})),
            Some("auth_context_mismatch")
        );
        assert_eq!(
            desktop_error_code(&json!({"error": {"code": "auth_context_mismatch"}})),
            Some("auth_context_mismatch")
        );
    }
    #[test]
    fn printing_keeps_bounded_per_format_failure_reasons() {
        let receipt =
            "Combined report not published\n- xlsx: built\n- pdf__a0: failed — table does not fit";
        assert_eq!(operation_error_detail("printing.export", receipt), receipt);
        assert_eq!(
            operation_error_detail("map.view", receipt),
            "Combined report not published"
        );
        assert_eq!(
            operation_error_detail("printing.export", &"é".repeat(3_000))
                .chars()
                .count(),
            2_000
        );
        assert!(
            !operation_error_detail("printing.export", "bad\u{001b}message").contains('\u{001b}')
        );
    }
}
