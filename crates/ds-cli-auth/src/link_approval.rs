//! Desktop-principal approval for one exact headless device authorization.
//!
//! The CLI receives no Desktop credential. It asks the paired application to
//! preview one fixed operation, verifies the returned public binding, and only
//! then asks that same application to commit the operator-confirmed approval.

use std::time::Duration;

use ds_cli_contract::spec::{
    Arg, Authority, Chapter, Command, Effect, Example, Execution, Refusal,
};
use ds_cli_contract::{Context, Failure, Inputs};
use ds_cli_desktop::ops::{self, BridgeOp, DESCRIPTOR_ARG};
use serde_json::{Value, json};

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
    when: "the Desktop preview does not exactly match the supplied request, fingerprint, or lane",
    remedy: "do not approve; compare the values on both devices and start a new request if needed",
};
const RECEIPT_UNREADABLE: Refusal = Refusal {
    code: "device_authorization_response_unreadable",
    when: "the paired application returns an incomplete or malformed approval receipt",
    remedy: "update DS GridDesign and ds to matching releases, then start a new request",
};

pub static COMMAND: Command = Command {
    id: "auth.link.approve",
    path: &["auth", "link", "approve"],
    contract: 1,
    chapter: Chapter::Project,
    summary: "Approve one exact headless device through paired Desktop (needs --yes).",
    purpose: "Fetches a read-only approval preview through the paired Desktop, checks its exact request, public-key fingerprint, lane, scopes, expiry, and renewable flag, then commits the same decision only after explicit confirmation. No Desktop credential or device private key enters ds.",
    effect: Effect::GlobalWrite,
    authority: Authority::DesktopUser,
    execution: Execution::Sync,
    args: &[REQUEST_ID, DEVICE_FINGERPRINT, LANE, DESCRIPTOR_ARG],
    output: "A bounded public approval receipt: request, decision, device name/platform/fingerprint, scopes, lane/profile/catalog binding, and times. Never a principal credential, device private key, proof, or token.",
    examples: &[Example {
        command: "ds auth link approve --request req_01 --device-fingerprint sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef --lane stable --yes --output json",
        note: "Compare the request and fingerprint on both devices before confirming.",
        runnable: false,
    }],
    refusals: &[
        ops::NOT_PAIRED,
        ops::AMBIGUOUS,
        ops::UNREACHABLE,
        ops::PAIRING_REJECTED,
        ops::REFUSED,
        ops::UNSUPPORTED,
        ops::UNREADABLE,
        ops::SIGNED_OUT,
        INVALID_REQUEST,
        BINDING_MISMATCH,
        RECEIPT_UNREADABLE,
    ],
    reference: Some("docs/contracts/unified-identity.md"),
    availability: ops::paired_availability,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let request_id = bounded_request(inputs.require("request")?)?;
    let fingerprint = bounded_fingerprint(inputs.require("device-fingerprint")?)?;
    let lane = inputs.require("lane")?;
    let found = ds_cli_desktop::bridge::paired(inputs.value("desktop-descriptor"))?;
    approve_with(request_id, fingerprint, lane, found.profile, |confirm| {
        invoke(&found.descriptor, request_id, fingerprint, confirm)
            .map_err(ops::classify_signed_out)
    })
}

fn approve_with<F>(
    request_id: &str,
    fingerprint: &str,
    lane: &str,
    desktop_profile: &str,
    mut invoke: F,
) -> Result<Value, Failure>
where
    F: FnMut(bool) -> Result<Value, Failure>,
{
    if matches!(desktop_profile, "stable" | "canary") && desktop_profile != lane {
        return Err(binding_mismatch(
            "the paired Desktop lane differs from --lane",
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
            "the paired Desktop approval receipt did not preserve the exact public binding",
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
        "the paired Desktop returned an invalid device approval receipt",
    )
    .remedy("update DS GridDesign and ds to matching releases, then start a new request")
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
