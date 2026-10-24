//! Self-service deletion of the signed-in account
//! (ds-brain `docs/contracts/account-deletion.md`).
//!
//! The account deleted is the restored identity the gateway verifies; the
//! request is closed to the one fixed confirmation phrase, so no caller input
//! can name another account. The caller's own confirmation step (`--yes`, a
//! dialog) happens before this owner is reached. The route is idempotent:
//! an interrupted deletion is finished by sending it again.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::{
    authorized::Authority, AuthorizedTransport, ClientError, ClientProfile, ErrorKind,
    ServiceRefusal,
};

/// The fixed phrase ds-brain requires after an explicit confirmation.
pub const CONFIRMATION: &str = "DELETE_MY_ACCOUNT";
/// The receipt schema this owner reads.
pub const SCHEMA: &str = "ds-account-deletion/v1";
/// The largest receipt this owner will read.
pub const MAX_RESPONSE_BYTES: usize = 64 * 1024;
/// The deletion walks the whole personal tree; the gateway allows 120 s.
pub const TIMEOUT_SECONDS: u64 = 120;

/// What one deletion removed, across retries. Replies may add members.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Counts {
    pub profile_documents: u64,
    pub project_memberships: u64,
    pub storage_objects: u64,
    pub device_credentials: u64,
    pub messaging_documents: u64,
    pub sign_in_account: bool,
}

/// The audited deletion receipt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Receipt {
    pub schema: String,
    pub receipt_id: String,
    pub state: String,
    pub already_deleted: bool,
    pub started_at: String,
    pub deleted_at: String,
    pub deleted: Counts,
    pub retained: Vec<String>,
}

/// One authenticated request to the account deletion route.
pub struct AccountDeletionCall<'a> {
    pub(crate) profile: &'a ClientProfile,
    pub(crate) authority: crate::transport::CallAuthority<'a>,
}

impl AccountDeletionCall<'_> {
    pub(crate) const GATEWAY_OPERATION: ds_gateway_client::Operation =
        ds_gateway_client::Operation::DeleteOwnAccount;

    crate::gateway_compat::gateway_common_accessors!(standard);
    crate::gateway_compat::gateway_fixed_route!(borrowed);

    pub const fn response_limit(&self) -> usize {
        MAX_RESPONSE_BYTES
    }
    pub const fn timeout_seconds(&self) -> u64 {
        TIMEOUT_SECONDS
    }

    pub fn body(&self) -> crate::SecretRequestBody {
        crate::SecretRequestBody::new(payload().to_string().into_bytes())
    }
}

/// The complete request body: there is nothing else it can carry.
pub fn payload() -> Value {
    json!({ "confirmation": CONFIRMATION })
}

fn unreadable() -> ClientError {
    ClientError::new(
        ErrorKind::UnreadableResponse,
        "account_delete_response_unreadable",
    )
}

pub(crate) fn run<T: AuthorizedTransport>(
    profile: &ClientProfile,
    transport: &mut T,
    authority: Authority<'_>,
) -> Result<Receipt, ClientError> {
    let response = transport.account_deletion(AccountDeletionCall {
        profile,
        authority: authority.call(),
    })?;
    if response.status != 200 {
        return Err(refusal(response.status, &response.body));
    }
    if response.body.len() > MAX_RESPONSE_BYTES {
        return Err(unreadable());
    }
    let envelope: Value = serde_json::from_slice(&response.body).map_err(|_| unreadable())?;
    if envelope["success"] != true {
        return Err(unreadable());
    }
    let receipt: Receipt =
        serde_json::from_value(envelope["data"].clone()).map_err(|_| unreadable())?;
    if receipt.schema != SCHEMA
        || receipt.state != "deleted"
        || receipt.receipt_id.is_empty()
        || !receipt.deleted.sign_in_account
    {
        return Err(unreadable());
    }
    Ok(receipt)
}

/// One non-200 answer as a classified error that keeps the route's own code
/// and scalar details (`step`, `retryable`), never its body.
pub fn refusal(status: u16, body: &[u8]) -> ClientError {
    let envelope = (body.len() <= MAX_RESPONSE_BYTES)
        .then(|| serde_json::from_slice::<Value>(body).ok())
        .flatten();
    let error = envelope.as_ref().map(|e| &e["error"]);
    let code = error.and_then(|e| e["code"].as_str());
    let message = error.and_then(|e| e["message"].as_str());
    let details = error.and_then(|e| e["details"].as_object());
    let kind = match (status, code) {
        (400, _) => ErrorKind::InvalidInput,
        (401 | 403, _) => ErrorKind::AuthenticationRejected,
        // A gateway that was never taught the route answers 404 with no code.
        (404, None) => ErrorKind::RouteUnavailable,
        (404, Some(_)) => ErrorKind::ResourceNotFound,
        (408 | 425 | 429 | 500..=599, _) => ErrorKind::Transient,
        _ => ErrorKind::UnreadableResponse,
    };
    ClientError::with_service_refusal(
        kind,
        "account_delete_refused",
        ServiceRefusal::with_details(status, code, message, details),
    )
}

#[cfg(test)]
#[path = "account_deletion_tests.rs"]
mod tests;
