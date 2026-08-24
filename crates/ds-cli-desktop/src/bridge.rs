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

use ds_cli_contract::outcome::Failure;
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
    timeout: Duration,
) -> Result<Value, Failure> {
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
        .send_json(json!({ "operation": operation, "arguments": arguments }));

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
        _ => Err(Failure::failed(
            "desktop_refused",
            format!("the paired session refused `{operation}`"),
        )
        .remedy("read detail for the application's own message")
        .detail(json!({
            "http_status": status,
            "detail": bounded(parsed["error"].as_str().unwrap_or(&body))
        }))),
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
