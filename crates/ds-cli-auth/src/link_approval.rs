//! Approval of one exact headless device authorization.
//!
//! `POST /api/v1/auth/device/approve` needs a Firebase USER id token and
//! nothing else — exactly what `ds auth login --email` leaves in the native
//! refresh store. So the restored native session is the approving principal:
//! it previews one fixed operation, this module verifies the returned public
//! binding, and only then does it commit the operator-confirmed approval. This
//! was the last governance write bound to a window: until 2026-09-19 the
//! Desktop was merely the only host that had ever held the token, so the estate
//! could be policed and revoked from a server but never joined from one.
//!
//! The paired Desktop remains the route for a machine with no native session.
//! A device credential is never the principal here — ds-brain refuses it with
//! `DESKTOP_PRINCIPAL_REQUIRED`, and the kernel deliberately has no device twin
//! of the call — because a device may not admit a sibling.

use std::time::Duration;

use ds_cli_contract::spec::{
    Arg, Authority, Chapter, Command, Effect, Example, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Failure, Inputs};
use ds_cli_desktop::ops::{self, BridgeOp, DESCRIPTOR_ARG};
use ds_client_core::{
    Client, ClientError, DeviceApprovalDecision, DeviceApprovalRequest, DeviceApprovalServiceCode,
    ErrorKind, RefreshTokenStore, Transport,
};
use serde_json::{Value, json};

use crate::profile::{self, Lane};
use crate::state::NativeRefreshStore;
use crate::transport::NativeTransport;

const TIMEOUT: Duration = Duration::from_secs(30);
const REQUEST_ID: Arg = Arg::value(
    "request",
    "<request-id>",
    "Exact public request id returned by `auth link begin`.",
)
.required();
const DEVICE_FINGERPRINT: Arg = Arg::value(
    "device-fingerprint",
    "<sha256:hex>",
    "Exact public device fingerprint displayed by the requesting device.",
)
.required();
const LANE: Arg = Arg::value(
    "lane",
    "<stable|canary>",
    "Deployment lane bound to the authorization request.",
)
.default("stable")
.choices(&["stable", "canary"]);

pub const APPROVE_OP: BridgeOp = BridgeOp {
    operation: "auth.link.approve",
    arguments: &["request_id", "device_fingerprint", "decision", "confirm"],
};

const INVALID_REQUEST: Refusal = Refusal {
    code: "device_authorization_input_invalid",
    when: "the request id or device fingerprint is empty, untrimmed, oversized, or malformed",
    remedy: "copy both exact public values from `ds auth link begin`",
};
const BINDING_MISMATCH: Refusal = Refusal {
    code: "device_authorization_binding_mismatch",
    when: "the approval preview does not exactly match the supplied request, fingerprint, or lane",
    remedy: "do not approve; compare the values on both devices and start a new request if needed",
};
const RECEIPT_UNREADABLE: Refusal = Refusal {
    code: "device_authorization_response_unreadable",
    when: "the approving session returns an incomplete or malformed approval receipt",
    remedy: "update ds, and DS GridDesign when paired, to matching releases, then start a new request",
};
// The route's own refusals, each with the next move ds-brain's
// `deviceauth/errors.go` names for it.
const CANNOT_ADMIT: Refusal = Refusal {
    code: "device_cannot_admit",
    when: "the signed-in credential is a device, and a device cannot admit a sibling",
    remedy: "approve from a session signed in with a password: `ds auth login --email <address> --lane <lane>`",
};
const NOT_FOUND: Refusal = Refusal {
    code: "device_authorization_not_found",
    when: "no authorization request carries this id on this lane; it may have begun on another",
    remedy: "run `ds auth link begin` again on the requesting device",
};
const EXPIRED: Refusal = Refusal {
    code: "device_authorization_expired",
    when: "the request expired deliberately and cannot be extended",
    remedy: "run `ds auth link begin` again on the requesting device",
};
const DECIDED: Refusal = Refusal {
    code: "device_authorization_decided",
    when: "the request was already decided, so the decision sent was not applied",
    remedy: "read its status with `ds auth link status` before deciding again",
};
const AUTHORITY_UNAVAILABLE: Refusal = Refusal {
    code: "device_authority_unavailable",
    when: "this lane's ds-brain carries no device authorization environment",
    remedy: "deploy it with the DEVICE_AUTH_* set from the canonical secrets bundle; no call against this revision can succeed",
};

pub static COMMAND: Command = Command {
    id: "auth.link.approve",
    path: &["auth", "link", "approve"],
    contract: 1,
    chapter: Chapter::Project,
    summary: "Approve one headless device as the signed-in user (needs --yes).",
    purpose: "Previews one approval under the restored native user, checks its exact request, fingerprint, lane, scopes, expiry and renewable flag, then commits the same decision only after explicit confirmation. A lane with no native session asks the paired Desktop instead. No credential or device private key enters ds; a device can never admit a sibling.",
    effect: Effect::GlobalWrite,
    authority: Authority::HeadlessUser,
    execution: Execution::Sync,
    args: &[REQUEST_ID, DEVICE_FINGERPRINT, LANE, DESCRIPTOR_ARG],
    output: "A bounded public approval receipt: request, decision, device name/platform/fingerprint, scopes, lane/profile/catalog binding and times; never a credential, private key, proof or token.",
    examples: &[Example {
        command: "ds auth link approve --request req_01 --device-fingerprint sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef --lane stable --yes --output json",
        note: "Compare the request and fingerprint on both devices before confirming.",
        runnable: false,
    }],
    refusals: &[
        super::SIGNED_OUT_REFUSAL,
        super::AUTH_REJECTED_REFUSAL,
        super::AUTH_REVOKED_REFUSAL,
        super::TRANSIENT_REFUSAL,
        INVALID_REQUEST,
        BINDING_MISMATCH,
        RECEIPT_UNREADABLE,
        CANNOT_ADMIT,
        NOT_FOUND,
        EXPIRED,
        DECIDED,
        AUTHORITY_UNAVAILABLE,
        // The paired route, reached only when this lane holds no native session.
        ops::NOT_PAIRED,
        ops::AMBIGUOUS,
        ops::UNREACHABLE,
        ops::PAIRING_REJECTED,
        ops::REFUSED,
        ops::UNSUPPORTED,
        ops::UNREADABLE,
        ops::SIGNED_OUT,
    ],
    reference: Some("docs/contracts/unified-identity.md"),
    search: &[],
    requires: Requires::Server,
    availability: super::native_availability,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let request_id = bounded_request(inputs.require("request")?)?;
    let fingerprint = bounded_fingerprint(inputs.require("device-fingerprint")?)?;
    let lane = inputs.require("lane")?;
    let profile = profile::load(Lane::parse(lane)?)?;
    match approver(NativeRefreshStore::probe(&profile)?.is_some()) {
        Approver::NativeSession => {
            let mut client = Client::new(profile, NativeTransport, NativeRefreshStore::open()?);
            crate::require_restore_before_context(&mut client)?;
            approve_natively(&mut client, request_id, fingerprint, lane)
        }
        Approver::PairedDesktop => {
            let found = ds_cli_desktop::bridge::paired(inputs.value("desktop-descriptor"))?;
            approve_with(request_id, fingerprint, lane, found.profile, |confirm| {
                invoke(&found.descriptor, request_id, fingerprint, confirm)
                    .map_err(ops::classify_signed_out)
            })
        }
    }
}

/// Who decides. The native session first, because the route needs nothing a
/// window has; the paired Desktop only when this lane holds no native session
/// at all, so a signed-in server never falls through to a window it lacks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Approver {
    NativeSession,
    PairedDesktop,
}

fn approver(native_session: bool) -> Approver {
    if native_session {
        Approver::NativeSession
    } else {
        Approver::PairedDesktop
    }
}

/// The two-step approval under the restored native user, through the kernel's
/// verified call. The approving principal's lane is the one its profile is
/// bound to, checked against `--lane` exactly as a paired Desktop's is.
fn approve_natively<T: Transport, S: RefreshTokenStore>(
    client: &mut Client<T, S>,
    request_id: &str,
    fingerprint: &str,
    lane: &str,
) -> Result<Value, Failure> {
    let principal_lane = client.profile().lane().token();
    approve_with(request_id, fingerprint, lane, principal_lane, |confirm| {
        let request = DeviceApprovalRequest::new(
            request_id,
            fingerprint,
            DeviceApprovalDecision::Approve,
            confirm,
        )
        .map_err(convert)?;
        client
            .device_approve(&request, crate::now())
            .map(|receipt| receipt.public_value())
            .map_err(convert)
    })
}

/// One kernel refusal as the code this command declares for it. The route's
/// own `deviceauth` code decides first; only a refusal that named none falls
/// back to its kind.
fn convert(error: ClientError) -> Failure {
    match error.device_approval_service_code() {
        Some(DeviceApprovalServiceCode::DesktopPrincipalRequired) => {
            Failure::unauthorized(CANNOT_ADMIT.code, CANNOT_ADMIT.when).remedy(CANNOT_ADMIT.remedy)
        }
        Some(DeviceApprovalServiceCode::FingerprintMismatch) => {
            binding_mismatch("the device fingerprint does not match the authorization request")
        }
        Some(DeviceApprovalServiceCode::ApprovalConflict) => {
            Failure::conflict(DECIDED.code, DECIDED.when)
                .remedy(DECIDED.remedy)
                .next("ds auth link status")
        }
        Some(DeviceApprovalServiceCode::AuthorizationExpired) => {
            Failure::invalid(EXPIRED.code, EXPIRED.when).remedy(EXPIRED.remedy)
        }
        Some(DeviceApprovalServiceCode::AuthorizationNotFound) => not_found(),
        // A property of the deployment, not of the request: no retry against
        // this revision can succeed, and the fault says so as ds-brain does.
        Some(DeviceApprovalServiceCode::AuthorityUnavailable) => {
            Failure::unavailable(AUTHORITY_UNAVAILABLE.code, AUTHORITY_UNAVAILABLE.when)
                .remedy(AUTHORITY_UNAVAILABLE.remedy)
                .detail(json!({ "fault": "deployment" }))
        }
        Some(DeviceApprovalServiceCode::RequestInvalid) => {
            invalid_input("the server's strict decoder refused the approval request")
        }
        None => match error.kind() {
            ErrorKind::InvalidInput => invalid_input(&error.to_string()),
            ErrorKind::ResourceNotFound => not_found(),
            ErrorKind::UnreadableResponse => receipt_unreadable(),
            _ => crate::map_client(error),
        },
    }
}

fn not_found() -> Failure {
    Failure::invalid(NOT_FOUND.code, NOT_FOUND.when).remedy(NOT_FOUND.remedy)
}

fn approve_with<F>(
    request_id: &str,
    fingerprint: &str,
    lane: &str,
    principal_lane: &str,
    mut invoke: F,
) -> Result<Value, Failure>
where
    F: FnMut(bool) -> Result<Value, Failure>,
{
    // The approving principal's own lane — the paired Desktop's profile lane,
    // or the native profile's — must be the one asked for.
    if matches!(principal_lane, "stable" | "canary") && principal_lane != lane {
        return Err(binding_mismatch(
            "the approving session's lane differs from --lane",
        ));
    }

    let preview = invoke(false)?;
    validate_receipt(&preview, request_id, fingerprint, lane, Phase::Preview)?;

    let receipt = invoke(true)?;
    validate_receipt(&receipt, request_id, fingerprint, lane, Phase::Committed)?;
    Ok(receipt)
}

fn invoke(
    descriptor: &ds_cli_desktop::discover::Descriptor,
    request_id: &str,
    fingerprint: &str,
    confirm: bool,
) -> Result<Value, Failure> {
    ops::invoke(
        descriptor,
        &APPROVE_OP,
        json!({
            "request_id": request_id,
            "device_fingerprint": fingerprint,
            "decision": "approve",
            "confirm": confirm,
        }),
        TIMEOUT,
    )
}

#[derive(Clone, Copy)]
enum Phase {
    Preview,
    Committed,
}

fn validate_receipt(
    value: &Value,
    request_id: &str,
    fingerprint: &str,
    lane: &str,
    phase: Phase,
) -> Result<(), Failure> {
    let object = value.as_object().ok_or_else(receipt_unreadable)?;
    let device = object
        .get("device")
        .and_then(Value::as_object)
        .ok_or_else(receipt_unreadable)?;
    let binding = object
        .get("binding")
        .and_then(Value::as_object)
        .ok_or_else(receipt_unreadable)?;
    let expected_preview = matches!(phase, Phase::Preview);
    let expected_status = if expected_preview {
        "pending"
    } else {
        "approved"
    };
    let exact = object.get("requestId").and_then(Value::as_str) == Some(request_id)
        && object.get("preview").and_then(Value::as_bool) == Some(expected_preview)
        && object.get("status").and_then(Value::as_str) == Some(expected_status)
        && object.get("renewable").and_then(Value::as_bool) == Some(true)
        && device.get("fingerprint").and_then(Value::as_str) == Some(fingerprint)
        && binding.get("lane").and_then(Value::as_str) == Some(lane)
        && object
            .get("scopes")
            .and_then(Value::as_array)
            .is_some_and(|scopes| scopes.len() == 1 && scopes[0].as_str() == Some("ds.api"));
    if !exact {
        return Err(binding_mismatch(
            "the approval receipt did not preserve the exact public binding",
        ));
    }
    if !bounded_public_text(device.get("name"), 128)
        || !matches!(
            device.get("platform").and_then(Value::as_str),
            Some("windows" | "linux" | "macos")
        )
        || !bounded_public_text(object.get("userCode"), 64)
        || !bounded_public_text(object.get("createdAt"), 64)
        || !bounded_public_text(object.get("expiresAt"), 64)
        || !valid_digest(binding.get("profileDigest"))
        || !valid_digest(binding.get("catalogDigest"))
        || !valid_raw_digest(binding.get("audience"))
    {
        return Err(receipt_unreadable());
    }
    if matches!(phase, Phase::Committed)
        && (object
            .get("decision")
            .and_then(Value::as_object)
            .and_then(|decision| decision.get("value"))
            .and_then(Value::as_str)
            != Some("approve"))
    {
        return Err(receipt_unreadable());
    }
    Ok(())
}

fn bounded_request(value: &str) -> Result<&str, Failure> {
    if value.is_empty()
        || value.trim() != value
        || value.len() > 128
        || value.chars().any(char::is_control)
    {
        return Err(invalid_input("--request is outside its public bound"));
    }
    Ok(value)
}

fn bounded_fingerprint(value: &str) -> Result<&str, Failure> {
    if !is_digest(value) {
        return Err(invalid_input(
            "--device-fingerprint must be sha256: followed by 64 lowercase hexadecimal digits",
        ));
    }
    Ok(value)
}

fn is_digest(value: &str) -> bool {
    value.len() == 71
        && value.starts_with("sha256:")
        && value[7..]
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn valid_digest(value: Option<&Value>) -> bool {
    value.and_then(Value::as_str).is_some_and(is_digest)
}

fn valid_raw_digest(value: Option<&Value>) -> bool {
    value.and_then(Value::as_str).is_some_and(|digest| {
        digest.len() == 64
            && digest
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    })
}

fn bounded_public_text(value: Option<&Value>, max: usize) -> bool {
    value.and_then(Value::as_str).is_some_and(|text| {
        !text.is_empty()
            && text.trim() == text
            && text.len() <= max
            && !text.chars().any(char::is_control)
    })
}

fn invalid_input(message: &str) -> Failure {
    Failure::invalid("device_authorization_input_invalid", message)
        .remedy("copy the exact request id and fingerprint from `ds auth link begin`")
}

fn binding_mismatch(message: &str) -> Failure {
    Failure::conflict("device_authorization_binding_mismatch", message)
        .remedy("do not approve; compare both devices and start a new request if needed")
}

fn receipt_unreadable() -> Failure {
    Failure::unavailable(
        "device_authorization_response_unreadable",
        "the approving session returned an invalid device approval receipt",
    )
    .remedy(RECEIPT_UNREADABLE.remedy)
}

pub fn render(data: &Value) -> String {
    format!(
        "approved device {} ({})\n  request  {}\n  lane     {}\n  expires  {}",
        data["device"]["name"].as_str().unwrap_or("unknown"),
        data["device"]["fingerprint"].as_str().unwrap_or("unknown"),
        data["requestId"].as_str().unwrap_or("unknown"),
        data["binding"]["lane"].as_str().unwrap_or("unknown"),
        data["expiresAt"].as_str().unwrap_or("unknown"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn receipt(preview: bool) -> Value {
        json!({
            "requestId": "req_01",
            "status": if preview { "pending" } else { "approved" },
            "preview": preview,
            "userCode": "BLUE-OTTER",
            "createdAt": "2026-08-30T12:00:00Z",
            "expiresAt": "2026-08-30T12:10:00Z",
            "renewable": true,
            "scopes": ["ds.api"],
            "binding": {
                "lane": "stable",
                "audience": "c".repeat(64),
                "profileDigest": format!("sha256:{}", "a".repeat(64)),
                "catalogDigest": format!("sha256:{}", "b".repeat(64)),
            },
            "device": {
                "name": "operator-laptop",
                "platform": "linux",
                "fingerprint": format!("sha256:{}", "c".repeat(64)),
            },
            "decision": if preview { Value::Null } else { json!({ "value": "approve", "decidedAt": "2026-08-30T12:01:00Z" }) },
        })
    }

    #[test]
    fn preview_and_commit_require_the_same_exact_public_binding() {
        let fingerprint = format!("sha256:{}", "c".repeat(64));
        assert!(
            validate_receipt(
                &receipt(true),
                "req_01",
                &fingerprint,
                "stable",
                Phase::Preview,
            )
            .is_ok()
        );
        assert!(
            validate_receipt(
                &receipt(false),
                "req_01",
                &fingerprint,
                "stable",
                Phase::Committed,
            )
            .is_ok()
        );
        assert_eq!(
            validate_receipt(
                &receipt(true),
                "req_other",
                &fingerprint,
                "stable",
                Phase::Preview,
            )
            .unwrap_err()
            .code(),
            "device_authorization_binding_mismatch"
        );
    }

    #[test]
    fn malformed_fingerprints_never_reach_desktop() {
        for value in ["", "abc", &format!("sha256:{}", "A".repeat(64))] {
            assert_eq!(
                bounded_fingerprint(value).unwrap_err().code(),
                "device_authorization_input_invalid"
            );
        }
    }

    #[test]
    fn approval_rejects_noncanonical_credential_audience() {
        let fingerprint = format!("sha256:{}", "c".repeat(64));
        for audience in [json!("ds-native-client"), json!("A".repeat(64))] {
            let mut value = receipt(true);
            value["binding"]["audience"] = audience;
            assert_eq!(
                validate_receipt(&value, "req_01", &fingerprint, "stable", Phase::Preview)
                    .unwrap_err()
                    .code(),
                "device_authorization_response_unreadable"
            );
        }
    }

    /// `deviceauth.ApprovalReceipt` as ds-brain's `approvalReceipt()` fills
    /// it, inside `pkg/response.Success`'s envelope, for the sign-in fixture's
    /// principal. The kernel decodes it; this module then validates its public
    /// shape exactly as it validated the Desktop's.
    fn wire_receipt(preview: bool) -> Vec<u8> {
        let mut data = json!({
            "request": {
                "request_id": "req_01",
                "status": if preview { "pending" } else { "approved" },
                "device": {
                    "name": "Workstation",
                    "platform": "linux",
                    "fingerprint": format!("sha256:{}", "c".repeat(64)),
                },
                "user_code": "BLUE-OTTER",
                "scopes": ["ds.api"],
                "binding": {
                    "lane": "stable",
                    "audience": "c".repeat(64),
                    "profile_digest": format!("sha256:{}", "a".repeat(64)),
                    "catalog_digest": format!("sha256:{}", "b".repeat(64)),
                },
                "created_at": "2026-09-19T08:00:00Z",
                "expires_at": "2026-09-19T08:10:00Z",
                "renewable": true
            },
            "preview": preview,
            "principal": { "uid": "uid-1", "email": "user@example.com" }
        });
        if !preview {
            data["decision"] = json!({ "value": "approve", "decided_at": "2026-09-19T08:01:00Z" });
        }
        serde_json::to_vec(&json!({
            "success": true,
            "message": "Device authorization decided",
            "data": data,
            "timestamp": "2026-09-19T08:01:00Z",
            "service": "data-solutions-backend",
            "version": "1.0.0",
            "request_id": "corr-1"
        }))
        .unwrap()
    }

    /// `pkg/response.Error` carrying one `deviceauth` code and its remedy.
    fn wire_refusal(code: &str) -> Vec<u8> {
        serde_json::to_vec(&json!({
            "success": false,
            "error": {
                "code": code,
                "message": "server sentence",
                "details": { "remedy": "server remedy" },
                "timestamp": 1
            },
            "timestamp": "2026-09-19T08:01:00Z",
            "service": "data-solutions-backend",
            "version": "1.0.0"
        }))
        .unwrap()
    }

    /// The native session is the approving principal, and it needs no window:
    /// the preview and the commit both reach the route under the restored
    /// USER's id token, in that order, and the receipt the kernel verified is
    /// the one this module validates and returns.
    #[test]
    fn the_native_session_approves_without_a_window() {
        use crate::test_support::{FixtureTransport, SIGN_IN, signed_in};

        assert_eq!(approver(true), Approver::NativeSession);
        assert_eq!(approver(false), Approver::PairedDesktop);

        let fingerprint = format!("sha256:{}", "c".repeat(64));
        let transport = FixtureTransport::with_sign_in(SIGN_IN);
        transport.push_device_approve(200, &wire_receipt(true));
        transport.push_device_approve(200, &wire_receipt(false));
        let mut client = signed_in(transport.clone());

        let receipt = approve_natively(&mut client, "req_01", &fingerprint, "stable").unwrap();
        assert_eq!(receipt["status"], "approved");
        assert_eq!(receipt["preview"], false);
        assert_eq!(receipt["decision"]["value"], "approve");
        assert_eq!(receipt["device"]["fingerprint"], fingerprint);

        let calls = transport.calls();
        assert_eq!(calls.len(), 2, "{calls:?}");
        let user_token: Value = serde_json::from_slice(SIGN_IN).unwrap();
        let user_token = user_token["idToken"].as_str().unwrap();
        for call in &calls {
            assert!(
                call.starts_with(&format!(
                    "device_approve /api/v1/auth/device/approve {user_token} "
                )),
                "{call}"
            );
        }
        assert!(
            calls[0].ends_with(r#""decision":"approve","confirm":false}"#),
            "{}",
            calls[0]
        );
        assert!(
            calls[1].ends_with(r#""decision":"approve","confirm":true}"#),
            "{}",
            calls[1]
        );

        // A lane the native profile is not bound to is refused before any call.
        transport.push_device_approve(200, &wire_receipt(true));
        let error = approve_natively(&mut client, "req_01", &fingerprint, "canary").unwrap_err();
        assert_eq!(error.code(), "device_authorization_binding_mismatch");
        assert_eq!(transport.calls().len(), 2, "no call was made");
    }

    /// Every `deviceauth` refusal the route emits crosses as its own declared
    /// code with the next move ds-brain names for it; the one that is a
    /// property of the deployment says so and never says "retry".
    #[test]
    fn each_route_refusal_is_its_own_declared_code() {
        use crate::test_support::{FixtureTransport, NOW, SIGN_IN, signed_in};
        use ds_cli_contract::outcome::ExitClass;

        let cases: &[(u16, &str, &str, ExitClass)] = &[
            (
                403,
                "DESKTOP_PRINCIPAL_REQUIRED",
                "device_cannot_admit",
                ExitClass::Unauthorized,
            ),
            (
                404,
                "DEVICE_AUTHORIZATION_NOT_FOUND",
                "device_authorization_not_found",
                ExitClass::InvalidInput,
            ),
            (
                409,
                "DEVICE_FINGERPRINT_MISMATCH",
                "device_authorization_binding_mismatch",
                ExitClass::Conflict,
            ),
            (
                409,
                "DEVICE_APPROVAL_CONFLICT",
                "device_authorization_decided",
                ExitClass::Conflict,
            ),
            (
                410,
                "DEVICE_AUTHORIZATION_EXPIRED",
                "device_authorization_expired",
                ExitClass::InvalidInput,
            ),
            (
                400,
                "DEVICE_REQUEST_INVALID",
                "device_authorization_input_invalid",
                ExitClass::InvalidInput,
            ),
            (
                503,
                "DEVICE_AUTH_DISABLED",
                "device_authority_unavailable",
                ExitClass::Unavailable,
            ),
        ];
        let fingerprint = format!("sha256:{}", "c".repeat(64));
        let request = DeviceApprovalRequest::new(
            "req_01",
            &fingerprint,
            DeviceApprovalDecision::Approve,
            false,
        )
        .unwrap();
        let transport = FixtureTransport::with_sign_in(SIGN_IN);
        let mut client = signed_in(transport.clone());
        let declared: Vec<&str> = COMMAND
            .refusals
            .iter()
            .map(|refusal| refusal.code)
            .collect();

        for (status, wire, code, class) in cases {
            transport.push_device_approve(*status, &wire_refusal(wire));
            let error = client.device_approve(&request, NOW + 1).unwrap_err();
            let failure = convert(error);
            assert_eq!(failure.code(), *code, "{wire}");
            assert_eq!(failure.class(), *class, "{wire}");
            assert!(declared.contains(code), "{code} is not declared");
            let remedy = failure
                .remedy_text()
                .unwrap_or_else(|| panic!("{code} has no remedy"));
            assert!(
                !failure.message().contains("server sentence"),
                "{wire} leaked"
            );
            assert!(!remedy.contains("server remedy"), "{wire} leaked");
            match *wire {
                "DESKTOP_PRINCIPAL_REQUIRED" => {
                    assert!(
                        remedy.contains("ds auth login --email <address> --lane <lane>"),
                        "{remedy}"
                    );
                    assert!(
                        failure.message().contains("device cannot admit a sibling"),
                        "{}",
                        failure.message()
                    );
                }
                "DEVICE_AUTH_DISABLED" => {
                    assert!(!remedy.to_lowercase().contains("retry"), "{remedy}");
                    assert!(remedy.contains("deploy"), "{remedy}");
                    assert_eq!(
                        failure.detail_value(),
                        Some(&json!({ "fault": "deployment" }))
                    );
                }
                "DEVICE_APPROVAL_CONFLICT" => {
                    assert!(remedy.contains("ds auth link status"), "{remedy}");
                    assert_eq!(failure.next_commands(), ["ds auth link status"]);
                }
                "DEVICE_FINGERPRINT_MISMATCH" => {
                    assert!(remedy.starts_with("do not approve"), "{remedy}");
                }
                "DEVICE_AUTHORIZATION_NOT_FOUND" | "DEVICE_AUTHORIZATION_EXPIRED" => {
                    assert!(remedy.contains("ds auth link begin"), "{remedy}");
                }
                _ => {}
            }
        }

        // A refusal the route did not type keeps the shared mapping: a 502 is
        // the native transport being transiently unavailable, not a decision.
        transport.push_device_approve(502, b"bad gateway");
        let error = client.device_approve(&request, NOW + 1).unwrap_err();
        assert_eq!(convert(error).code(), "auth_transient");
    }

    /// The contract says what the route needs: a server with a signed-in
    /// native user, never a window. The paired route stays declared as the
    /// fallback it is.
    #[test]
    fn approval_is_declared_as_a_server_command_of_the_native_user() {
        assert_eq!(COMMAND.requires, Requires::Server);
        assert_eq!(COMMAND.authority, Authority::HeadlessUser);
        assert!(
            !COMMAND.summary.contains("paired Desktop"),
            "{}",
            COMMAND.summary
        );
        assert!(
            !COMMAND.purpose.contains("through the paired Desktop"),
            "{}",
            COMMAND.purpose
        );
        let declared: Vec<&str> = COMMAND
            .refusals
            .iter()
            .map(|refusal| refusal.code)
            .collect();
        for code in [
            "headless_signed_out",
            "device_cannot_admit",
            "device_authorization_not_found",
            "device_authorization_expired",
            "device_authorization_decided",
            "device_authority_unavailable",
            "desktop_not_paired",
        ] {
            assert!(declared.contains(&code), "{code} is not declared");
        }
        // The widest code sets the REFUSALS column every row pays for; a new
        // one may not widen it.
        let widest = declared.iter().map(|code| code.len()).max().unwrap();
        assert_eq!(widest, "device_authorization_response_unreadable".len());
    }

    #[test]
    fn approval_always_validates_preview_before_one_confirmed_mutation() {
        let fingerprint = format!("sha256:{}", "c".repeat(64));
        let mut phases = Vec::new();
        let result = approve_with("req_01", &fingerprint, "stable", "stable", |confirm| {
            phases.push(confirm);
            Ok(receipt(!confirm))
        })
        .unwrap();
        assert_eq!(phases, [false, true]);
        assert_eq!(result["status"], "approved");

        phases.clear();
        let error = approve_with("req_other", &fingerprint, "stable", "stable", |confirm| {
            phases.push(confirm);
            Ok(receipt(true))
        })
        .unwrap_err();
        assert_eq!(error.code(), "device_authorization_binding_mismatch");
        assert_eq!(phases, [false], "a failed preview must prevent mutation");
    }
}
