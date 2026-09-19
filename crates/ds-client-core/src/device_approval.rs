//! Approval of one headless device authorization, headlessly.
//!
//! `POST /api/v1/auth/device/approve` sits behind ds-brain's
//! `FirebaseAuthentication` boundary (`internal/middleware/middleware.go`),
//! whose only requirement is a Firebase USER id token with a uid and an email —
//! exactly what a headless `ds auth login --email` leaves in the native refresh
//! store. Nothing about approving needs a window; the Desktop's window was
//! merely the only host that had ever held the token. A device credential is
//! refused with `403 DESKTOP_PRINCIPAL_REQUIRED`, and that refusal crosses as
//! its own typed code: a device may not admit a sibling, and the operator must
//! be told it is the credential KIND that was refused. For the same reason
//! there is no twin on `DeviceApiAuthorization`.
//!
//! Closed by construction: [`DeviceApprovalRequest`] is the complete request
//! (`deviceauth.ApprovalRequest`, decoded with `DisallowUnknownFields` and a
//! 16 KiB limit), and the receipt is decoded against `deviceauth.ApprovalReceipt`
//! field for field, then checked the way the Desktop checks it before the CLI
//! is allowed to trust it.

use serde::Deserialize;
use serde_json::{json, Value};

use crate::device::{bounded, digest_value, validate_binding};
use crate::{
    authorized::Authority, AuthorizedTransport, ClientError, ClientProfile,
    DeviceApprovalServiceCode, DeviceApproveCall, DeviceAuthorizationStatus, DeviceBinding,
    DevicePrincipal, ErrorKind, TransportResponse, DEVICE_SCOPE,
};

/// The longest request id this owner will send. ds-brain's own bound.
pub const MAX_REQUEST_ID_BYTES: usize = 128;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceApprovalDecision {
    Approve,
    Deny,
}

impl DeviceApprovalDecision {
    pub const fn token(self) -> &'static str {
        match self {
            Self::Approve => "approve",
            Self::Deny => "deny",
        }
    }

    /// The request status the server records for this decision.
    const fn decided_status(self) -> DeviceAuthorizationStatus {
        match self {
            Self::Approve => DeviceAuthorizationStatus::Approved,
            Self::Deny => DeviceAuthorizationStatus::Denied,
        }
    }
}

/// One exact approval request: the four keys `deviceauth.ApprovalRequest`
/// decodes, and nothing else.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceApprovalRequest {
    request_id: String,
    device_fingerprint: String,
    decision: DeviceApprovalDecision,
    confirm: bool,
}

impl DeviceApprovalRequest {
    /// `confirm: false` asks for the read-only preview the operator compares
    /// against the requesting device; `confirm: true` commits the decision.
    pub fn new(
        request_id: &str,
        device_fingerprint: &str,
        decision: DeviceApprovalDecision,
        confirm: bool,
    ) -> Result<Self, ClientError> {
        bounded(request_id, MAX_REQUEST_ID_BYTES).map_err(|_| {
            invalid("the device authorization request id is empty, untrimmed, or outside its bound")
        })?;
        digest_value(device_fingerprint).map_err(|_| {
            invalid("the device fingerprint must be sha256: followed by 64 lowercase hex digits")
        })?;
        Ok(Self {
            request_id: request_id.to_owned(),
            device_fingerprint: device_fingerprint.to_owned(),
            decision,
            confirm,
        })
    }

    pub fn request_id(&self) -> &str {
        &self.request_id
    }
    pub fn device_fingerprint(&self) -> &str {
        &self.device_fingerprint
    }
    pub const fn decision(&self) -> DeviceApprovalDecision {
        self.decision
    }
    pub const fn confirm(&self) -> bool {
        self.confirm
    }

    pub(crate) fn payload(&self) -> Value {
        json!({
            "request_id": self.request_id,
            "device_fingerprint": self.device_fingerprint,
            "decision": self.decision.token(),
            "confirm": self.confirm,
        })
    }

    fn expected_status(&self) -> DeviceAuthorizationStatus {
        if self.confirm {
            self.decision.decided_status()
        } else {
            DeviceAuthorizationStatus::Pending
        }
    }
}

/// The device the request names, as the receipt describes it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceApprovalDevice {
    name: String,
    platform: String,
    fingerprint: String,
}

impl DeviceApprovalDevice {
    pub fn name(&self) -> &str {
        &self.name
    }
    pub fn platform(&self) -> &str {
        &self.platform
    }
    pub fn fingerprint(&self) -> &str {
        &self.fingerprint
    }
}

/// The recorded decision. Present on a committed receipt, absent on a preview.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceApprovalRecordedDecision {
    value: DeviceApprovalDecision,
    decided_at: String,
}

impl DeviceApprovalRecordedDecision {
    pub const fn value(&self) -> DeviceApprovalDecision {
        self.value
    }
    pub fn decided_at(&self) -> &str {
        &self.decided_at
    }
}

/// One verified approval receipt: the public binding of the request, checked
/// against what was sent and against the principal that sent it.
///
/// It carries no credential, proof or token — the server never returns one on
/// this route — and no principal: the receipt was verified to belong to the
/// caller, which is the only thing a host may learn about the principal here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceApprovalReceipt {
    request_id: String,
    status: DeviceAuthorizationStatus,
    preview: bool,
    user_code: String,
    created_at: String,
    expires_at: String,
    binding: DeviceBinding,
    device: DeviceApprovalDevice,
    decision: Option<DeviceApprovalRecordedDecision>,
}

impl DeviceApprovalReceipt {
    pub fn request_id(&self) -> &str {
        &self.request_id
    }
    pub const fn status(&self) -> DeviceAuthorizationStatus {
        self.status
    }
    pub const fn preview(&self) -> bool {
        self.preview
    }
    pub fn user_code(&self) -> &str {
        &self.user_code
    }
    pub fn created_at(&self) -> &str {
        &self.created_at
    }
    pub fn expires_at(&self) -> &str {
        &self.expires_at
    }
    pub fn binding(&self) -> &DeviceBinding {
        &self.binding
    }
    pub fn device(&self) -> &DeviceApprovalDevice {
        &self.device
    }
    pub fn decision(&self) -> Option<&DeviceApprovalRecordedDecision> {
        self.decision.as_ref()
    }

    /// The receipt in the shape the Desktop hands its callers
    /// (`ds-web/src/lib/auth/device-approval.ts`): camelCase, `renewable`
    /// and `scopes` restated as the constants the decode proved, `decision`
    /// only when one was recorded, and no principal. A host that validated a
    /// Desktop receipt validates this one unchanged.
    pub fn public_value(&self) -> Value {
        let mut value = json!({
            "requestId": self.request_id,
            "status": self.status.token(),
            "preview": self.preview,
            "userCode": self.user_code,
            "createdAt": self.created_at,
            "expiresAt": self.expires_at,
            "renewable": true,
            "scopes": [DEVICE_SCOPE],
            "binding": {
                "lane": self.binding.lane,
                "audience": self.binding.audience,
                "profileDigest": self.binding.profile_digest,
                "catalogDigest": self.binding.catalog_digest,
            },
            "device": {
                "name": self.device.name,
                "platform": self.device.platform,
                "fingerprint": self.device.fingerprint,
            },
        });
        if let Some(decision) = &self.decision {
            value["decision"] = json!({
                "value": decision.value.token(),
                "decidedAt": decision.decided_at,
            });
        }
        value
    }
}

// ---------------------------------------------------------------------------
// The wire, field for field from `internal/deviceauth/types.go`
// ---------------------------------------------------------------------------

/// The standard envelope is ds-brain's, not this route's; only `success` and
/// `data` are read from it.
#[derive(Deserialize)]
struct Envelope {
    #[serde(default)]
    success: bool,
    #[serde(default)]
    data: Option<ReceiptWire>,
}

/// `deviceauth.ApprovalReceipt`.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReceiptWire {
    request: RequestWire,
    preview: bool,
    /// `json:"decision,omitempty"` — absent on a preview.
    #[serde(default)]
    decision: Option<DecisionWire>,
    principal: DevicePrincipal,
}

/// The anonymous `Request` struct of `deviceauth.ApprovalReceipt`.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RequestWire {
    request_id: String,
    status: DeviceAuthorizationStatus,
    device: DeviceWire,
    user_code: String,
    scopes: Vec<String>,
    binding: DeviceBinding,
    created_at: String,
    expires_at: String,
    renewable: bool,
}

/// `deviceauth.ApprovalDevice`.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DeviceWire {
    name: String,
    platform: String,
    fingerprint: String,
}

/// `deviceauth.ApprovalDecision`.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DecisionWire {
    value: String,
    decided_at: String,
}

/// The half of ds-brain's error envelope a refusal is read from.
#[derive(Deserialize)]
struct ErrorEnvelope {
    #[serde(default)]
    error: Option<ErrorWire>,
}

#[derive(Deserialize)]
struct ErrorWire {
    #[serde(default)]
    code: String,
}

fn invalid(message: &'static str) -> ClientError {
    ClientError::new(ErrorKind::InvalidInput, message)
}

fn unreadable(message: &'static str) -> ClientError {
    ClientError::new(ErrorKind::UnreadableResponse, message)
}

/// Decode one `200` body into a verified receipt.
///
/// Every check the Desktop makes before it lets a receipt out
/// (`device-approval.ts`) is made here, so the CLI's own validator meets the
/// same guarantees whichever host produced the receipt.
pub(crate) fn decode_receipt(
    body: &[u8],
    request: &DeviceApprovalRequest,
    uid: &str,
    email: &str,
) -> Result<DeviceApprovalReceipt, ClientError> {
    const SHAPE: &str = "the device approval receipt is outside its closed contract";
    if body.len() > crate::transport::DEVICE_APPROVE_RESPONSE_LIMIT {
        return Err(unreadable(SHAPE));
    }
    let envelope: Envelope = serde_json::from_slice(body).map_err(|_| unreadable(SHAPE))?;
    let wire = match (envelope.success, envelope.data) {
        (true, Some(wire)) => wire,
        _ => return Err(unreadable(SHAPE)),
    };
    // Whose receipt this is comes first: a receipt for another account is not
    // a malformed one, it is the wrong one.
    if wire.principal.uid != uid || !wire.principal.email.eq_ignore_ascii_case(email) {
        return Err(unreadable(
            "the device approval receipt names another principal",
        ));
    }
    let received = wire.request;
    if received.request_id != request.request_id
        || received.device.fingerprint != request.device_fingerprint
    {
        return Err(unreadable(
            "the device approval receipt does not match the request that was sent",
        ));
    }
    if wire.preview != !request.confirm
        || received.status != request.expected_status()
        || !received.renewable
        || received.scopes.as_slice() != [DEVICE_SCOPE]
        || !matches!(
            received.device.platform.as_str(),
            "windows" | "linux" | "macos"
        )
    {
        return Err(unreadable(SHAPE));
    }
    for (value, max) in [
        (&received.user_code, 64),
        (&received.created_at, 64),
        (&received.expires_at, 64),
        (&received.device.name, 100),
    ] {
        bounded(value, max).map_err(|_| unreadable(SHAPE))?;
    }
    digest_value(&received.device.fingerprint).map_err(|_| unreadable(SHAPE))?;
    validate_binding(&received.binding).map_err(|_| unreadable(SHAPE))?;
    let decision = match (request.confirm, wire.decision) {
        (false, None) => None,
        (true, Some(decision)) => {
            bounded(&decision.decided_at, 64).map_err(|_| unreadable(SHAPE))?;
            if decision.value != request.decision.token() {
                return Err(unreadable(
                    "the device approval receipt does not match the decision that was sent",
                ));
            }
            Some(DeviceApprovalRecordedDecision {
                value: request.decision,
                decided_at: decision.decided_at,
            })
        }
        // A preview carrying a decision, or a commit without one.
        _ => return Err(unreadable(SHAPE)),
    };
    Ok(DeviceApprovalReceipt {
        request_id: received.request_id,
        status: received.status,
        preview: wire.preview,
        user_code: received.user_code,
        created_at: received.created_at,
        expires_at: received.expires_at,
        binding: received.binding,
        device: DeviceApprovalDevice {
            name: received.device.name,
            platform: received.device.platform,
            fingerprint: received.device.fingerprint,
        },
        decision,
    })
}

/// Classify one refusal. The route's own `deviceauth` code is matched exactly
/// against its status (`internal/deviceauth/errors.go`, `service.go`,
/// `handlers/device_auth.go`); anything else falls back to the status alone.
pub(crate) fn refusal(response: &TransportResponse) -> ClientError {
    let code = if response.body.len() <= crate::transport::DEVICE_APPROVE_RESPONSE_LIMIT {
        serde_json::from_slice::<ErrorEnvelope>(&response.body)
            .ok()
            .and_then(|envelope| envelope.error)
            .map(|error| error.code)
            .unwrap_or_default()
    } else {
        String::new()
    };
    let typed = match (response.status, code.as_str()) {
        (403, "DESKTOP_PRINCIPAL_REQUIRED") => Some((
            DeviceApprovalServiceCode::DesktopPrincipalRequired,
            ErrorKind::AuthenticationRejected,
            "device approval needs a signed-in user; a device credential cannot approve a sibling",
        )),
        (404, "DEVICE_AUTHORIZATION_NOT_FOUND") => Some((
            DeviceApprovalServiceCode::AuthorizationNotFound,
            ErrorKind::ResourceNotFound,
            "no device authorization request carries this id on this lane",
        )),
        (409, "DEVICE_FINGERPRINT_MISMATCH") => Some((
            DeviceApprovalServiceCode::FingerprintMismatch,
            ErrorKind::InvalidInput,
            "the device fingerprint does not match the authorization request; do not approve",
        )),
        (409, "DEVICE_APPROVAL_CONFLICT") => Some((
            DeviceApprovalServiceCode::ApprovalConflict,
            ErrorKind::InvalidInput,
            "the device authorization request was already decided; read its status",
        )),
        (410, "DEVICE_AUTHORIZATION_EXPIRED") => Some((
            DeviceApprovalServiceCode::AuthorizationExpired,
            ErrorKind::ResourceNotFound,
            "the device authorization request has expired; begin it again",
        )),
        (400, "DEVICE_REQUEST_INVALID") => Some((
            DeviceApprovalServiceCode::RequestInvalid,
            ErrorKind::InvalidInput,
            "the device approval request was refused by the server's strict decoder",
        )),
        (503, "DEVICE_AUTH_DISABLED") => Some((
            DeviceApprovalServiceCode::AuthorityUnavailable,
            ErrorKind::RouteUnavailable,
            "this deployment carries no device authorization environment; no retry can succeed",
        )),
        _ => None,
    };
    if let Some((code, kind, message)) = typed {
        return ClientError::with_device_approval_service_code(kind, message, code);
    }
    match response.status {
        404 => ClientError::new(
            ErrorKind::ResourceNotFound,
            "no such device authorization request",
        ),
        400 | 409 | 410 => invalid("the device approval was refused for this request"),
        408 | 425 | 429 | 500..=599 => ClientError::new(
            ErrorKind::Transient,
            "device approval is temporarily unavailable",
        ),
        _ => ClientError::new(
            ErrorKind::AuthenticationRejected,
            "device approval refused the verified user",
        ),
    }
}

pub(crate) fn run<T: AuthorizedTransport>(
    profile: &ClientProfile,
    transport: &mut T,
    authority: Authority<'_>,
    request: &DeviceApprovalRequest,
) -> Result<DeviceApprovalReceipt, ClientError> {
    let response = transport.device_approve(DeviceApproveCall {
        profile,
        payload: request.payload(),
        authority: authority.call(),
    })?;
    if response.status != 200 {
        return Err(refusal(&response));
    }
    decode_receipt(&response.body, request, authority.uid, authority.email)
}

#[cfg(test)]
mod tests {
    use super::*;

    const FINGERPRINT: &str =
        "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    fn request(decision: DeviceApprovalDecision, confirm: bool) -> DeviceApprovalRequest {
        DeviceApprovalRequest::new("req_01", FINGERPRINT, decision, confirm).unwrap()
    }

    /// `deviceauth.ApprovalReceipt` as `approvalReceipt()` fills it, inside
    /// `pkg/response.Success`'s envelope. `decision` is `omitempty`.
    fn body(status: &str, preview: bool, decision: Option<&str>) -> Vec<u8> {
        let mut data = json!({
            "request": {
                "request_id": "req_01",
                "status": status,
                "device": {"name": "Workstation", "platform": "linux", "fingerprint": FINGERPRINT},
                "user_code": "ABCD-EFGH",
                "scopes": ["ds.api"],
                "binding": {
                    "lane": "stable",
                    "audience": "d".repeat(64),
                    "profile_digest": format!("sha256:{}", "a".repeat(64)),
                    "catalog_digest": format!("sha256:{}", "b".repeat(64)),
                },
                "created_at": "2026-09-19T08:00:00Z",
                "expires_at": "2026-09-19T08:10:00Z",
                "renewable": true
            },
            "preview": preview,
            "principal": {"uid": "uid-1", "email": "user@example.com"}
        });
        if let Some(value) = decision {
            data["decision"] = json!({"value": value, "decided_at": "2026-09-19T08:01:00Z"});
        }
        serde_json::to_vec(&json!({
            "success": true,
            "message": if preview { "Device approval preview" } else { "Device authorization decided" },
            "data": data,
            "timestamp": "2026-09-19T08:01:00Z",
            "service": "data-solutions-backend",
            "version": "1.0.0",
            "request_id": "corr-1"
        }))
        .unwrap()
    }

    fn error_body(code: &str) -> Vec<u8> {
        serde_json::to_vec(&json!({
            "success": false,
            "error": {"code": code, "message": "server sentence", "details": {"remedy": "..."}, "timestamp": 1},
            "timestamp": "2026-09-19T08:01:00Z",
            "service": "data-solutions-backend",
            "version": "1.0.0"
        }))
        .unwrap()
    }

    #[test]
    fn the_request_is_the_four_keys_the_backend_decoder_accepts() {
        let payload = request(DeviceApprovalDecision::Deny, true).payload();
        let mut keys: Vec<&str> = payload
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        keys.sort_unstable();
        assert_eq!(
            keys,
            ["confirm", "decision", "device_fingerprint", "request_id"]
        );
        assert_eq!(payload["decision"], "deny");
        assert_eq!(payload["confirm"], true);

        assert!(DeviceApprovalRequest::new(
            "",
            FINGERPRINT,
            DeviceApprovalDecision::Approve,
            false
        )
        .is_err());
        assert!(DeviceApprovalRequest::new(
            " req",
            FINGERPRINT,
            DeviceApprovalDecision::Approve,
            false
        )
        .is_err());
        assert!(DeviceApprovalRequest::new(
            &"r".repeat(129),
            FINGERPRINT,
            DeviceApprovalDecision::Approve,
            false
        )
        .is_err());
        assert!(DeviceApprovalRequest::new(
            "req_01",
            &FINGERPRINT[7..],
            DeviceApprovalDecision::Approve,
            false
        )
        .is_err());
        assert!(DeviceApprovalRequest::new(
            "req_01",
            &FINGERPRINT.to_uppercase(),
            DeviceApprovalDecision::Approve,
            false
        )
        .is_err());
    }

    #[test]
    fn a_preview_and_a_commit_decode_to_the_desktops_public_shape() {
        let preview = decode_receipt(
            &body("pending", true, None),
            &request(DeviceApprovalDecision::Approve, false),
            "uid-1",
            "User@Example.com",
        )
        .unwrap();
        assert!(preview.preview());
        assert_eq!(preview.status(), DeviceAuthorizationStatus::Pending);
        assert!(preview.decision().is_none());
        let public = preview.public_value();
        let mut keys: Vec<&str> = public
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        keys.sort_unstable();
        // Exactly what `device-approval.ts` emits and what the CLI's
        // `validate_receipt` reads; no `principal`, no `decision` on a preview.
        assert_eq!(
            keys,
            [
                "binding",
                "createdAt",
                "device",
                "expiresAt",
                "preview",
                "renewable",
                "requestId",
                "scopes",
                "status",
                "userCode"
            ]
        );
        assert_eq!(public["requestId"], "req_01");
        assert_eq!(public["status"], "pending");
        assert_eq!(public["renewable"], true);
        assert_eq!(public["scopes"], json!(["ds.api"]));
        assert_eq!(
            public["binding"]["profileDigest"],
            format!("sha256:{}", "a".repeat(64))
        );
        assert_eq!(
            public["binding"]["catalogDigest"],
            format!("sha256:{}", "b".repeat(64))
        );
        assert_eq!(public["binding"]["audience"], "d".repeat(64));
        assert_eq!(public["binding"]["lane"], "stable");
        assert_eq!(public["device"]["fingerprint"], FINGERPRINT);
        assert_eq!(public["device"]["platform"], "linux");

        let committed = decode_receipt(
            &body("approved", false, Some("approve")),
            &request(DeviceApprovalDecision::Approve, true),
            "uid-1",
            "user@example.com",
        )
        .unwrap();
        assert_eq!(committed.status(), DeviceAuthorizationStatus::Approved);
        assert_eq!(
            committed.decision().unwrap().value(),
            DeviceApprovalDecision::Approve
        );
        let public = committed.public_value();
        assert_eq!(public["status"], "approved");
        assert_eq!(public["preview"], false);
        assert_eq!(public["decision"]["value"], "approve");
        assert_eq!(public["decision"]["decidedAt"], "2026-09-19T08:01:00Z");

        let denied = decode_receipt(
            &body("denied", false, Some("deny")),
            &request(DeviceApprovalDecision::Deny, true),
            "uid-1",
            "user@example.com",
        )
        .unwrap();
        assert_eq!(denied.public_value()["decision"]["value"], "deny");
    }

    #[test]
    fn a_receipt_that_is_not_this_request_or_this_principal_is_refused() {
        let approve = request(DeviceApprovalDecision::Approve, false);
        let refused = |body: &[u8], uid: &str, email: &str, request: &DeviceApprovalRequest| {
            decode_receipt(body, request, uid, email).unwrap_err()
        };
        let pending = body("pending", true, None);
        assert_eq!(
            refused(&pending, "uid-2", "user@example.com", &approve).to_string(),
            "the device approval receipt names another principal"
        );
        assert_eq!(
            refused(&pending, "uid-1", "other@example.com", &approve).to_string(),
            "the device approval receipt names another principal"
        );
        let other = DeviceApprovalRequest::new(
            "req_02",
            FINGERPRINT,
            DeviceApprovalDecision::Approve,
            false,
        )
        .unwrap();
        assert_eq!(
            refused(&pending, "uid-1", "user@example.com", &other).to_string(),
            "the device approval receipt does not match the request that was sent"
        );
        // Phase and status must be the ones this request implies.
        assert!(decode_receipt(
            &pending,
            &request(DeviceApprovalDecision::Approve, true),
            "uid-1",
            "user@example.com"
        )
        .is_err());
        assert!(decode_receipt(
            &body("approved", true, None),
            &approve,
            "uid-1",
            "user@example.com"
        )
        .is_err());
        // A preview may not carry decision metadata; a commit must.
        assert!(decode_receipt(
            &body("pending", true, Some("approve")),
            &approve,
            "uid-1",
            "user@example.com"
        )
        .is_err());
        assert!(decode_receipt(
            &body("approved", false, None),
            &request(DeviceApprovalDecision::Approve, true),
            "uid-1",
            "user@example.com"
        )
        .is_err());
        assert_eq!(
            refused(
                &body("approved", false, Some("deny")),
                "uid-1",
                "user@example.com",
                &request(DeviceApprovalDecision::Approve, true)
            )
            .to_string(),
            "the device approval receipt does not match the decision that was sent"
        );
        // The receipt is closed: a key the Go type does not declare is a
        // different contract.
        let mut loose: Value = serde_json::from_slice(&pending).unwrap();
        loose["data"]["request"]["access_token"] = json!("never");
        assert!(decode_receipt(
            &serde_json::to_vec(&loose).unwrap(),
            &approve,
            "uid-1",
            "user@example.com"
        )
        .is_err());
        let mut loose: Value = serde_json::from_slice(&pending).unwrap();
        loose["data"]["credential"] = json!({});
        assert!(decode_receipt(
            &serde_json::to_vec(&loose).unwrap(),
            &approve,
            "uid-1",
            "user@example.com"
        )
        .is_err());
        let mut wrong_scope: Value = serde_json::from_slice(&pending).unwrap();
        wrong_scope["data"]["request"]["scopes"] = json!(["ds.api", "ds.admin"]);
        assert!(decode_receipt(
            &serde_json::to_vec(&wrong_scope).unwrap(),
            &approve,
            "uid-1",
            "user@example.com"
        )
        .is_err());
        let mut not_renewable: Value = serde_json::from_slice(&pending).unwrap();
        not_renewable["data"]["request"]["renewable"] = json!(false);
        assert!(decode_receipt(
            &serde_json::to_vec(&not_renewable).unwrap(),
            &approve,
            "uid-1",
            "user@example.com"
        )
        .is_err());
        assert!(decode_receipt(
            br#"{"success":false}"#,
            &approve,
            "uid-1",
            "user@example.com"
        )
        .is_err());
    }

    #[test]
    fn every_refusal_the_route_authors_crosses_as_its_own_typed_code() {
        let cases = [
            (
                403,
                "DESKTOP_PRINCIPAL_REQUIRED",
                DeviceApprovalServiceCode::DesktopPrincipalRequired,
                ErrorKind::AuthenticationRejected,
            ),
            (
                404,
                "DEVICE_AUTHORIZATION_NOT_FOUND",
                DeviceApprovalServiceCode::AuthorizationNotFound,
                ErrorKind::ResourceNotFound,
            ),
            (
                409,
                "DEVICE_FINGERPRINT_MISMATCH",
                DeviceApprovalServiceCode::FingerprintMismatch,
                ErrorKind::InvalidInput,
            ),
            (
                409,
                "DEVICE_APPROVAL_CONFLICT",
                DeviceApprovalServiceCode::ApprovalConflict,
                ErrorKind::InvalidInput,
            ),
            (
                410,
                "DEVICE_AUTHORIZATION_EXPIRED",
                DeviceApprovalServiceCode::AuthorizationExpired,
                ErrorKind::ResourceNotFound,
            ),
            (
                400,
                "DEVICE_REQUEST_INVALID",
                DeviceApprovalServiceCode::RequestInvalid,
                ErrorKind::InvalidInput,
            ),
            (
                503,
                "DEVICE_AUTH_DISABLED",
                DeviceApprovalServiceCode::AuthorityUnavailable,
                ErrorKind::RouteUnavailable,
            ),
        ];
        for (status, code, expected, kind) in cases {
            let error = refusal(&TransportResponse::new(status, error_body(code)));
            assert_eq!(
                error.device_approval_service_code(),
                Some(expected),
                "{code}"
            );
            assert_eq!(error.kind(), kind, "{code}");
            assert!(!error.to_string().contains("server sentence"), "{code}");
        }
        // A code on the wrong status is not that code.
        let error = refusal(&TransportResponse::new(
            401,
            error_body("DESKTOP_PRINCIPAL_REQUIRED"),
        ));
        assert_eq!(error.device_approval_service_code(), None);
        assert_eq!(error.kind(), ErrorKind::AuthenticationRejected);
        // The status alone still classifies a refusal the route did not name.
        assert_eq!(
            refusal(&TransportResponse::new(502, b"")).kind(),
            ErrorKind::Transient
        );
        assert_eq!(
            refusal(&TransportResponse::new(404, b"{}")).kind(),
            ErrorKind::ResourceNotFound
        );
        assert_eq!(
            refusal(&TransportResponse::new(409, b"not json")).kind(),
            ErrorKind::InvalidInput
        );
    }
}
