//! The signed-in account: binding its Firebase UID to the platform directory,
//! its own profile, the capabilities it holds, and its decision on one
//! pending device authorization. Brain proves the bearer and owns every
//! record; these operations name the request and admit the answer.

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

use super::{data, project_reference, Admitted, Exchange, Method, Plan, Refusal, Request};

const INFO_PATH: &str = "/api/v1/user/info";
const PROFILE_PATH: &str = "/api/v1/user/profile";
const CAPABILITIES_PATH: &str = "/api/v1/user/capabilities";
const DEVICE_APPROVE_PATH: &str = "/api/v1/auth/device/approve";
const INFO_TIMEOUT_MS: u64 = 15_000;
/// Resolving a large account's project grants can take minutes.
const CAPABILITIES_TIMEOUT_MS: u64 = 120_000;
const PROFILE_FIELDS_MAX: usize = 64;
const EMAIL_MAX_CHARS: usize = 320;
pub const INVALID: &str = "account_answer_invalid";
pub const INPUT_INVALID: &str = "account_input_invalid";
pub const DEVICE_RECEIPT_INVALID: &str = "device_approval_receipt_invalid";

fn invalid(what: &str) -> Refusal {
    Refusal::new(
        INVALID,
        format!("The account service answered an invalid {what}."),
    )
}

fn canonical_email(email: &str) -> Option<String> {
    let valid = (3..=EMAIL_MAX_CHARS).contains(&email.chars().count())
        && email.trim() == email
        && !email.chars().any(char::is_control);
    valid.then(|| email.to_lowercase())
}

/// Bind this session's Firebase UID to its directory entry: answers whether
/// Brain proved this exact UID and email and recorded the binding.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IdentityRepair {
    pub uid: String,
    pub email: String,
}

impl Exchange for IdentityRepair {
    type Output = bool;

    fn plan(&self) -> Result<Plan, Refusal> {
        if self.uid.is_empty()
            || self.uid.trim() != self.uid
            || canonical_email(&self.email).is_none()
        {
            return Err(Refusal::new(
                INPUT_INVALID,
                "An identity repair names the session's UID and email.",
            ));
        }
        let request = Request {
            method: Method::Get,
            body: None,
            ..Request::post(INFO_PATH, Value::Null)
        };
        Ok(super::Plan {
            request: request.timeout_ms(INFO_TIMEOUT_MS),
            retries: 1,
        })
    }

    fn admit(self, _status: u16, body: Value) -> Result<Admitted<Self>, Refusal> {
        let answer = data(&body);
        let text = |key: &str| answer.get(key).and_then(Value::as_str);
        Ok(Admitted::Done(
            text("uid") == Some(self.uid.as_str())
                && text("email").and_then(canonical_email) == canonical_email(&self.email)
                && answer.get("firestore_updated").and_then(Value::as_bool) == Some(true),
        ))
    }
}

/// The account's own profile document; Brain names its fields.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Profile;

fn profile_of(body: &Value) -> Result<Map<String, Value>, Refusal> {
    data(body)
        .as_object()
        .cloned()
        .ok_or_else(|| invalid("profile"))
}

impl Exchange for Profile {
    type Output = Map<String, Value>;

    fn plan(&self) -> Result<Plan, Refusal> {
        Ok(Request {
            method: Method::Get,
            body: None,
            ..Request::post(PROFILE_PATH, Value::Null)
        }
        .into())
    }

    fn admit(self, _status: u16, body: Value) -> Result<Admitted<Self>, Refusal> {
        profile_of(&body).map(Admitted::Done)
    }
}

/// Write fields of the account's own profile; Brain keeps only the fields it
/// allows and answers the profile as stored.
#[derive(Clone, Debug, PartialEq)]
pub struct ProfileWrite {
    pub fields: Map<String, Value>,
}

impl Exchange for ProfileWrite {
    type Output = Map<String, Value>;

    fn plan(&self) -> Result<Plan, Refusal> {
        if self.fields.is_empty() || self.fields.len() > PROFILE_FIELDS_MAX {
            return Err(Refusal::new(
                INPUT_INVALID,
                "A profile write names 1 to 64 fields.",
            ));
        }
        Ok(Request {
            method: Method::Put,
            ..Request::post(PROFILE_PATH, Value::Object(self.fields.clone()))
        }
        .into())
    }

    fn admit(self, _status: u16, body: Value) -> Result<Admitted<Self>, Refusal> {
        profile_of(&body).map(Admitted::Done)
    }
}

/// The capabilities the account holds, for the platform or one project.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Capabilities {
    pub project: Option<String>,
}

impl Exchange for Capabilities {
    type Output = Map<String, Value>;

    fn plan(&self) -> Result<Plan, Refusal> {
        let body = match &self.project {
            Some(project) => json!({"project_id": project_reference(project)?}),
            None => json!({}),
        };
        Ok(Request::post(CAPABILITIES_PATH, body)
            .timeout_ms(CAPABILITIES_TIMEOUT_MS)
            .into())
    }

    fn admit(self, _status: u16, body: Value) -> Result<Admitted<Self>, Refusal> {
        let capabilities = data(&body)
            .as_object()
            .filter(|snapshot| snapshot.contains_key("app") || snapshot.contains_key("user"))
            .cloned()
            .ok_or_else(|| invalid("capability snapshot"))?;
        Ok(Admitted::Done(capabilities))
    }

    fn project(&self) -> Option<&str> {
        self.project.as_deref()
    }
}

/// One decision on a pending device authorization, as the person states it.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), derive(tsify::Tsify))]
#[serde(deny_unknown_fields)]
pub struct DeviceDecision {
    pub request_id: String,
    pub device_fingerprint: String,
    /// `approve` or `deny`.
    pub decision: String,
    /// `false` previews the request; `true` records the decision.
    pub confirm: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), derive(tsify::Tsify))]
pub struct DeviceBinding {
    pub lane: String,
    pub audience: String,
    pub profile_digest: String,
    pub catalog_digest: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), derive(tsify::Tsify))]
pub struct ApprovalDevice {
    pub name: String,
    pub platform: String,
    pub fingerprint: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), derive(tsify::Tsify))]
pub struct ApprovalDecision {
    pub value: String,
    pub decided_at: String,
}

/// What a preview shows or a decision recorded, admitted only when it is
/// this request, this device and this account.
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), derive(tsify::Tsify))]
pub struct DeviceApprovalReceipt {
    pub request_id: String,
    /// `pending` on a preview; `approved` or `denied` once decided.
    pub status: String,
    pub preview: bool,
    pub user_code: String,
    pub created_at: String,
    pub expires_at: String,
    pub scopes: Vec<String>,
    pub binding: DeviceBinding,
    pub device: ApprovalDevice,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), tsify(optional))]
    pub decision: Option<ApprovalDecision>,
}

#[derive(Deserialize)]
struct ReceiptRequest {
    request_id: String,
    status: String,
    device: ApprovalDevice,
    user_code: String,
    scopes: Vec<String>,
    binding: DeviceBinding,
    created_at: String,
    expires_at: String,
    renewable: bool,
}

#[derive(Deserialize)]
struct ReceiptPrincipal {
    uid: String,
    email: String,
}

#[derive(Deserialize)]
struct Receipt {
    request: ReceiptRequest,
    preview: bool,
    #[serde(default)]
    decision: Option<ApprovalDecision>,
    principal: ReceiptPrincipal,
}

fn bounded(text: &str, max: usize) -> bool {
    (1..=max).contains(&text.chars().count())
        && text.trim() == text
        && !text.chars().any(char::is_control)
}

fn hex(text: &str, length: usize) -> bool {
    text.len() == length
        && text
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn digest(text: &str) -> bool {
    text.strip_prefix("sha256:")
        .is_some_and(|value| hex(value, 64))
}

/// Preview or record one device decision as the signed-in account.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeviceApproval {
    pub decision: DeviceDecision,
    pub uid: String,
    pub email: String,
}

impl DeviceApproval {
    fn receipt_invalid(what: &str) -> Refusal {
        Refusal::new(
            DEVICE_RECEIPT_INVALID,
            format!("The device authorization receipt {what}."),
        )
    }

    fn expected_status(&self) -> &'static str {
        match (self.decision.confirm, self.decision.decision.as_str()) {
            (false, _) => "pending",
            (true, "approve") => "approved",
            (true, _) => "denied",
        }
    }
}

impl Exchange for DeviceApproval {
    type Output = DeviceApprovalReceipt;

    fn plan(&self) -> Result<Plan, Refusal> {
        let decision = &self.decision;
        if !bounded(&decision.request_id, 128)
            || !digest(&decision.device_fingerprint)
            || !["approve", "deny"].contains(&decision.decision.as_str())
        {
            return Err(Refusal::new(
                INPUT_INVALID,
                "A device decision names one request, its device fingerprint and approve or deny.",
            ));
        }
        if self.uid.is_empty() || canonical_email(&self.email).is_none() {
            return Err(Refusal::new(
                INPUT_INVALID,
                "Sign in before approving a device.",
            ));
        }
        let body = serde_json::to_value(decision).map_err(|_| invalid("device decision"))?;
        Ok(Request::post(DEVICE_APPROVE_PATH, body).into())
    }

    fn admit(self, _status: u16, body: Value) -> Result<Admitted<Self>, Refusal> {
        let receipt = Receipt::deserialize(data(&body))
            .map_err(|_| Self::receipt_invalid("is unreadable"))?;
        if receipt.principal.uid != self.uid
            || canonical_email(&receipt.principal.email) != canonical_email(&self.email)
        {
            return Err(Self::receipt_invalid("belongs to another account"));
        }
        let request = receipt.request;
        if request.request_id != self.decision.request_id
            || request.device.fingerprint != self.decision.device_fingerprint
        {
            return Err(Self::receipt_invalid("does not match this request"));
        }
        if receipt.preview == self.decision.confirm {
            return Err(Self::receipt_invalid("is of the wrong phase"));
        }
        if request.status != self.expected_status() || !request.renewable {
            return Err(Self::receipt_invalid("has an invalid status"));
        }
        if request.scopes != ["ds.api"] {
            return Err(Self::receipt_invalid("has invalid scopes"));
        }
        let binding = &request.binding;
        let device = &request.device;
        if !["windows", "linux", "macos"].contains(&device.platform.as_str())
            || !bounded(&device.name, 100)
            || !bounded(&request.user_code, 64)
            || !bounded(&request.created_at, 64)
            || !bounded(&request.expires_at, 64)
            || !bounded(&binding.lane, 32)
            || !hex(&binding.audience, 64)
            || !digest(&binding.profile_digest)
            || !digest(&binding.catalog_digest)
        {
            return Err(Self::receipt_invalid("is malformed"));
        }
        let decision = match (self.decision.confirm, receipt.decision) {
            (true, Some(decided))
                if decided.value == self.decision.decision && bounded(&decided.decided_at, 64) =>
            {
                Some(decided)
            }
            (false, None) => None,
            _ => return Err(Self::receipt_invalid("does not match this decision")),
        };
        Ok(Admitted::Done(DeviceApprovalReceipt {
            request_id: request.request_id,
            status: request.status,
            preview: receipt.preview,
            user_code: request.user_code,
            created_at: request.created_at,
            expires_at: request.expires_at,
            scopes: request.scopes,
            binding: request.binding,
            device: request.device,
            decision,
        }))
    }
}

/// The typed browser exports Svelte calls; each runs the shared executor.
#[cfg(all(feature = "wasm", target_arch = "wasm32"))]
mod exports {
    use super::{
        Capabilities, DeviceApproval, DeviceDecision, IdentityRepair, Profile, ProfileWrite,
    };
    use crate::gateway_exchange::browser::{attempt, run, typed, GatewayHost};
    use serde_json::{Map, Value};
    use wasm_bindgen::prelude::*;

    fn unreadable(error: impl std::fmt::Display) -> JsValue {
        js_sys::Error::new(&error.to_string()).into()
    }

    /// Bind this session's UID to its directory entry; answers whether Brain
    /// proved and recorded it. A refusal answers `false`.
    #[wasm_bindgen(js_name = accountIdentityRepair)]
    pub async fn account_identity_repair(
        host: GatewayHost,
        uid: String,
        email: String,
    ) -> Result<bool, JsValue> {
        Ok(matches!(
            attempt(&host, IdentityRepair { uid, email }).await?,
            Ok(true)
        ))
    }

    /// The account's own profile document.
    #[wasm_bindgen(js_name = accountProfile, unchecked_return_type = "Record<string, unknown>")]
    pub async fn account_profile(host: GatewayHost) -> Result<JsValue, JsValue> {
        typed(&run(&host, Profile).await?)
    }

    /// Write fields of the account's own profile; answers the stored profile.
    #[wasm_bindgen(js_name = accountProfileWrite, unchecked_return_type = "Record<string, unknown>")]
    pub async fn account_profile_write(
        host: GatewayHost,
        #[wasm_bindgen(unchecked_param_type = "Record<string, unknown>")] fields: JsValue,
    ) -> Result<JsValue, JsValue> {
        let fields: Map<String, Value> =
            tsify::serde_wasm_bindgen::from_value(fields).map_err(unreadable)?;
        typed(&run(&host, ProfileWrite { fields }).await?)
    }

    /// The capabilities the account holds, for the platform or one project.
    #[wasm_bindgen(js_name = accountCapabilities, unchecked_return_type = "Record<string, unknown>")]
    pub async fn account_capabilities(
        host: GatewayHost,
        project: Option<String>,
    ) -> Result<JsValue, JsValue> {
        typed(&run(&host, Capabilities { project }).await?)
    }

    /// Preview or record one device decision as the signed-in account.
    #[wasm_bindgen(js_name = deviceApproval, unchecked_return_type = "DeviceApprovalReceipt")]
    pub async fn device_approval(
        host: GatewayHost,
        #[wasm_bindgen(unchecked_param_type = "DeviceDecision")] decision: JsValue,
        uid: String,
        email: String,
    ) -> Result<JsValue, JsValue> {
        let decision: DeviceDecision =
            tsify::serde_wasm_bindgen::from_value(decision).map_err(unreadable)?;
        typed(
            &run(
                &host,
                DeviceApproval {
                    decision,
                    uid,
                    email,
                },
            )
            .await?,
        )
    }
}

#[cfg(test)]
#[path = "account_tests.rs"]
mod tests;
