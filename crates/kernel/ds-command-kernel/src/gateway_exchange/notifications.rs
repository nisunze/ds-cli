//! The account's notification center and its browser push registrations:
//! one page of canonical notifications, read and dismiss marks, the push
//! configuration, and registering or revoking one push subscription. Brain
//! owns the rows and the push stack; these operations bound each request and
//! admit only well-formed rows, so a cached row can never carry an unsafe
//! route.

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

use super::{data, Admitted, Exchange, Plan, Refusal, Request};

const PATH: &str = "/api/v1/notifications";
/// Brain's own page and mutation bounds.
pub const MAX_PAGE_SIZE: u32 = 100;
pub const MAX_IDS_PER_MUTATION: usize = 100;
const ROUTE_MAX_CHARS: usize = 512;
const ID_MAX_CHARS: usize = 256;
const ENDPOINT_MAX_CHARS: usize = 2_048;
pub const INVALID: &str = "notifications_answer_invalid";
pub const INPUT_INVALID: &str = "notifications_input_invalid";
/// Brain's code for a deployment without a push stack.
pub const PUSH_NOT_CONFIGURED: &str = "PUSH_NOT_CONFIGURED";

fn invalid() -> Refusal {
    Refusal::new(
        INVALID,
        "The notification service answered an invalid result.",
    )
}

fn input_invalid(message: &str) -> Refusal {
    Refusal::new(INPUT_INVALID, message)
}

/// A rooted same-origin path: never absolute, protocol-relative, backslashed
/// or carrying control characters. Brain sanitises routes the same way; a row
/// held from before a fix is checked again here.
pub fn safe_route(route: &str) -> bool {
    let value = route.trim();
    !value.is_empty()
        && value.chars().count() <= ROUTE_MAX_CHARS
        && value.starts_with('/')
        && !value.starts_with("//")
        && !value.contains('\\')
        && !value.chars().any(char::is_control)
}

/// What a release-update notification names.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), derive(tsify::Tsify))]
pub struct ReleaseUpdate {
    pub component: String,
    pub lane: String,
    pub product_id: String,
    pub version: String,
    pub platform: String,
    pub arch: String,
    pub protocol: String,
    pub sha256: String,
}

/// How a notification is drawn.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), derive(tsify::Tsify))]
#[serde(rename_all = "snake_case")]
pub enum NotificationSeverity {
    Info,
    Success,
    Warning,
    Error,
}

/// One canonical notification.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), derive(tsify::Tsify))]
pub struct CanonicalNotification {
    pub id: String,
    pub severity: NotificationSeverity,
    pub title: String,
    /// Epoch milliseconds, server-stamped.
    pub created_at: i64,
    pub read: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), tsify(optional))]
    pub kind: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), tsify(optional))]
    pub body: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), tsify(optional))]
    pub project_id: Option<String>,
    /// Present only when it is a safe same-origin route.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), tsify(optional))]
    pub route: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), tsify(optional))]
    pub source: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), tsify(optional))]
    pub release_update: Option<ReleaseUpdate>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), tsify(optional))]
    pub read_at: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), tsify(optional))]
    pub schema: Option<i64>,
}

fn trimmed(value: Option<&Value>) -> Option<String> {
    value
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(str::to_owned)
}

/// One row as the center holds it, or nothing when it is malformed: a row
/// without an id or title would corrupt the center's merge and sort.
pub fn canonical(row: &Value) -> Option<CanonicalNotification> {
    let id = trimmed(row.get("id"))?;
    let title = trimmed(row.get("title"))?;
    let severity = match row.get("severity").and_then(Value::as_str) {
        Some("success") => NotificationSeverity::Success,
        Some("warning") => NotificationSeverity::Warning,
        Some("error") => NotificationSeverity::Error,
        _ => NotificationSeverity::Info,
    };
    Some(CanonicalNotification {
        id,
        severity,
        title,
        created_at: row.get("created_at").and_then(Value::as_i64).unwrap_or(0),
        read: row.get("read").and_then(Value::as_bool) == Some(true),
        kind: trimmed(row.get("kind")),
        body: trimmed(row.get("body")),
        project_id: trimmed(row.get("project_id")),
        route: row
            .get("route")
            .and_then(Value::as_str)
            .filter(|route| safe_route(route))
            .map(str::to_owned),
        source: trimmed(row.get("source")),
        release_update: row
            .get("release_update")
            .and_then(|update| ReleaseUpdate::deserialize(update).ok()),
        read_at: row.get("read_at").and_then(Value::as_i64),
        schema: row.get("schema").and_then(Value::as_i64),
    })
}

/// One page of the center, newest first.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), derive(tsify::Tsify))]
pub struct NotificationPage {
    pub notifications: Vec<CanonicalNotification>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), tsify(optional))]
    pub next_cursor: Option<String>,
    pub has_more: bool,
    /// The page size Brain applied.
    pub limit: u32,
}

/// One page of notifications after `cursor`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Page {
    pub limit: Option<u32>,
    pub cursor: Option<String>,
}

impl Exchange for Page {
    type Output = NotificationPage;

    fn plan(&self) -> Result<Plan, Refusal> {
        let mut body = json!({"action": "list"});
        if let Some(limit) = self.limit {
            if !(1..=MAX_PAGE_SIZE).contains(&limit) {
                return Err(input_invalid("A notification page holds 1 to 100 rows."));
            }
            body["limit"] = json!(limit);
        }
        if let Some(cursor) = self.cursor.as_deref().filter(|cursor| !cursor.is_empty()) {
            body["cursor"] = json!(cursor);
        }
        Ok(Request::post(PATH, body).into())
    }

    fn admit(self, _status: u16, body: Value) -> Result<Admitted<Self>, Refusal> {
        let answer = data(&body);
        let rows = answer
            .get("notifications")
            .and_then(Value::as_array)
            .ok_or_else(invalid)?;
        let notifications: Vec<_> = rows.iter().filter_map(canonical).collect();
        Ok(Admitted::Done(NotificationPage {
            next_cursor: trimmed(answer.get("next_cursor")),
            has_more: answer.get("has_more").and_then(Value::as_bool) == Some(true),
            limit: answer
                .get("limit")
                .and_then(Value::as_u64)
                .map_or(notifications.len() as u32, |limit| limit as u32),
            notifications,
        }))
    }
}

/// A read or dismiss mark on up to [`MAX_IDS_PER_MUTATION`] notifications.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), derive(tsify::Tsify))]
#[serde(rename_all = "snake_case")]
pub enum NotificationMark {
    MarkRead,
    Dismiss,
}

/// What one mark changed; replaying it changes nothing.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), derive(tsify::Tsify))]
pub struct MarkOutcome {
    pub requested: u32,
    pub changed: u32,
    pub unchanged: u32,
    pub missing: u32,
}

/// Mark one chunk of notifications.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Marked {
    pub mark: NotificationMark,
    pub ids: Vec<String>,
}

impl Exchange for Marked {
    type Output = MarkOutcome;

    fn plan(&self) -> Result<Plan, Refusal> {
        if self.ids.is_empty()
            || self.ids.len() > MAX_IDS_PER_MUTATION
            || self
                .ids
                .iter()
                .any(|id| id.trim().is_empty() || id.chars().count() > ID_MAX_CHARS)
        {
            return Err(input_invalid("A mark names 1 to 100 notifications."));
        }
        Ok(Request::post(PATH, json!({"action": self.mark, "ids": self.ids})).into())
    }

    fn admit(self, _status: u16, body: Value) -> Result<Admitted<Self>, Refusal> {
        MarkOutcome::deserialize(data(&body))
            .map(Admitted::Done)
            .map_err(|_| invalid())
    }
}

/// The ids, in request-sized chunks.
pub fn chunks(ids: &[String]) -> Vec<Vec<String>> {
    ids.chunks(MAX_IDS_PER_MUTATION)
        .map(<[String]>::to_vec)
        .collect()
}

/// Which ids a chunked mark settled, and which it could not.
#[derive(Clone, Debug, Default, Serialize, PartialEq, Eq)]
#[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), derive(tsify::Tsify))]
pub struct MarkResult {
    pub succeeded: Vec<String>,
    pub failed: Vec<String>,
    /// At least one chunk failed; the center should reconcile.
    pub partial: bool,
}

/// One registered browser, never its endpoint or keys.
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), derive(tsify::Tsify))]
pub struct PushSubscriptionSummary {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), tsify(optional))]
    pub host: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), tsify(optional))]
    pub user_agent: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), tsify(optional))]
    pub created_at: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), tsify(optional))]
    pub updated_at: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), tsify(optional))]
    pub last_status: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), tsify(optional))]
    pub last_reason: Option<String>,
}

fn summary(row: &Value) -> Option<PushSubscriptionSummary> {
    Some(PushSubscriptionSummary {
        id: trimmed(row.get("id"))?,
        host: trimmed(row.get("host")),
        user_agent: trimmed(row.get("user_agent")),
        created_at: row.get("created_at").and_then(Value::as_i64),
        updated_at: row.get("updated_at").and_then(Value::as_i64),
        last_status: row.get("last_status").and_then(Value::as_i64),
        last_reason: trimmed(row.get("last_reason")),
    })
}

/// The deployment's push configuration and the account's registered browsers.
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), derive(tsify::Tsify))]
pub struct PushConfig {
    pub enabled: bool,
    /// For `PushManager.subscribe()`; empty when push is off.
    pub vapid_public_key: String,
    pub payload_schema_version: i64,
    pub max_subscriptions: i64,
    pub notification_cap: i64,
    pub max_page_size: i64,
    pub default_page_size: i64,
    pub max_ids_per_mutation: i64,
    pub subscriptions: Vec<PushSubscriptionSummary>,
}

/// The push configuration. A deployment without a push stack refuses with
/// [`PUSH_NOT_CONFIGURED`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Config;

impl Exchange for Config {
    type Output = PushConfig;

    fn plan(&self) -> Result<Plan, Refusal> {
        Ok(Request::post(PATH, json!({"action": "push_config"})).into())
    }

    fn admit(self, _status: u16, body: Value) -> Result<Admitted<Self>, Refusal> {
        let answer = data(&body);
        let number = |key: &str| answer.get(key).and_then(Value::as_i64).unwrap_or(0);
        Ok(Admitted::Done(PushConfig {
            enabled: answer.get("enabled").and_then(Value::as_bool) == Some(true),
            vapid_public_key: trimmed(answer.get("vapid_public_key")).unwrap_or_default(),
            payload_schema_version: number("payload_schema_version"),
            max_subscriptions: number("max_subscriptions"),
            notification_cap: number("notification_cap"),
            max_page_size: number("max_page_size"),
            default_page_size: number("default_page_size"),
            max_ids_per_mutation: number("max_ids_per_mutation"),
            subscriptions: answer
                .get("subscriptions")
                .and_then(Value::as_array)
                .map(|rows| rows.iter().filter_map(summary).collect())
                .unwrap_or_default(),
        }))
    }
}

/// One browser push subscription as `PushSubscription.toJSON()` gives it.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), derive(tsify::Tsify))]
pub struct PushSubscriptionKeys {
    pub p256dh: String,
    pub auth: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), derive(tsify::Tsify))]
pub struct BrowserPushSubscription {
    pub endpoint: String,
    pub keys: PushSubscriptionKeys,
}

/// What a registration recorded, and the key Brain signs with now.
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), derive(tsify::Tsify))]
pub struct PushRegistered {
    pub subscription: PushSubscriptionSummary,
    pub vapid_public_key: String,
}

/// Register one browser push subscription for the account.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Register {
    pub subscription: BrowserPushSubscription,
}

fn endpoint_valid(endpoint: &str) -> bool {
    endpoint.starts_with("https://")
        && endpoint.chars().count() <= ENDPOINT_MAX_CHARS
        && !endpoint.chars().any(char::is_control)
}

impl Exchange for Register {
    type Output = PushRegistered;

    fn plan(&self) -> Result<Plan, Refusal> {
        let subscription = &self.subscription;
        if !endpoint_valid(&subscription.endpoint)
            || subscription.keys.p256dh.trim().is_empty()
            || subscription.keys.auth.trim().is_empty()
        {
            return Err(input_invalid(
                "A push subscription names an https endpoint and its keys.",
            ));
        }
        let body = json!({"action": "register_push", "subscription": subscription});
        Ok(Request::post(PATH, body).into())
    }

    fn admit(self, _status: u16, body: Value) -> Result<Admitted<Self>, Refusal> {
        let answer = data(&body);
        Ok(Admitted::Done(PushRegistered {
            subscription: answer
                .get("subscription")
                .and_then(summary)
                .ok_or_else(invalid)?,
            vapid_public_key: trimmed(answer.get("vapid_public_key")).unwrap_or_default(),
        }))
    }
}

/// Revoke one push subscription by its id or endpoint; answers whether this
/// call removed it (revoking a gone one is still a success).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Revoke {
    pub subscription_id: Option<String>,
    pub endpoint: Option<String>,
}

impl Exchange for Revoke {
    type Output = bool;

    fn plan(&self) -> Result<Plan, Refusal> {
        let mut body = Map::new();
        body.insert("action".into(), json!("revoke_push"));
        match (
            self.subscription_id
                .as_deref()
                .filter(|id| !id.trim().is_empty()),
            self.endpoint
                .as_deref()
                .filter(|endpoint| !endpoint.is_empty()),
        ) {
            (Some(id), None) if id.chars().count() <= ID_MAX_CHARS => {
                body.insert("subscription_id".into(), json!(id));
            }
            (None, Some(endpoint)) if endpoint_valid(endpoint) => {
                body.insert("endpoint".into(), json!(endpoint));
            }
            _ => {
                return Err(input_invalid(
                    "A revoke names one subscription id or one https endpoint.",
                ))
            }
        }
        Ok(Request::post(PATH, Value::Object(body)).into())
    }

    fn admit(self, _status: u16, body: Value) -> Result<Admitted<Self>, Refusal> {
        Ok(Admitted::Done(
            data(&body).get("revoked").and_then(Value::as_bool) == Some(true),
        ))
    }
}

/// One page of the center. The newest page is kept so the next open paints
/// it at once and offline; a later page is never kept.
#[cfg(feature = "durable-store")]
pub async fn page(
    edge: &impl super::retained::Edge,
    limit: Option<u32>,
    cursor: Option<String>,
) -> Result<NotificationPage, Refusal> {
    let kind = crate::retained_reads::Kind::NotificationsPage;
    let newest = cursor.is_none();
    let call = Page { limit, cursor };
    if newest {
        super::retained::keep_fresh(edge, kind, None, "", call).await
    } else {
        edge.fetch(call).await
    }
}

/// The newest page this device holds, with no transport.
#[cfg(feature = "durable-store")]
pub async fn held_page(edge: &impl super::retained::Edge) -> Option<NotificationPage> {
    let kind = crate::retained_reads::Kind::NotificationsPage;
    super::retained::kept(edge, kind, None, "")
        .await
        .map(|held| held.value)
}

/// The typed browser exports Svelte calls; each runs the shared executor.
#[cfg(all(feature = "wasm", target_arch = "wasm32"))]
mod exports {
    use super::{
        chunks, held_page, page, BrowserPushSubscription, Config, MarkResult, Marked,
        NotificationMark, Register, Revoke,
    };
    use crate::auth_session::browser::EndedSession;
    use crate::durable_store::port::StoreHost;
    use crate::gateway_exchange::browser::{
        attempt, attempt_as, rejection, run, typed, GatewayHost,
    };
    use crate::gateway_exchange::retained::browser::BrowserEdge;
    use wasm_bindgen::prelude::*;

    fn unreadable(error: impl std::fmt::Display) -> JsValue {
        js_sys::Error::new(&error.to_string()).into()
    }

    /// One page of the account's notifications, newest first; the newest is
    /// kept for the next open.
    #[wasm_bindgen(js_name = notificationsPage, unchecked_return_type = "NotificationPage")]
    pub async fn notifications_page(
        host: GatewayHost,
        store: StoreHost,
        limit: Option<u32>,
        cursor: Option<String>,
    ) -> Result<JsValue, JsValue> {
        let edge = BrowserEdge::new(host, store);
        typed(
            &page(&edge, limit, cursor)
                .await
                .map_err(|refusal| rejection(&refusal))?,
        )
    }

    /// The newest page this device holds, at once and offline.
    #[wasm_bindgen(
        js_name = notificationsHeld,
        unchecked_return_type = "NotificationPage | undefined"
    )]
    pub async fn notifications_held(host: GatewayHost, store: StoreHost) -> Result<JsValue, JsValue> {
        let edge = BrowserEdge::new(host, store);
        typed(&held_page(&edge).await)
    }

    /// Mark notifications read or dismissed, in request-sized chunks; answers
    /// which ids each chunk settled.
    #[wasm_bindgen(js_name = notificationsMark, unchecked_return_type = "MarkResult")]
    pub async fn notifications_mark(
        host: GatewayHost,
        #[wasm_bindgen(unchecked_param_type = "NotificationMark")] mark: JsValue,
        ids: Vec<String>,
    ) -> Result<JsValue, JsValue> {
        let mark: NotificationMark =
            tsify::serde_wasm_bindgen::from_value(mark).map_err(unreadable)?;
        let mut result = MarkResult::default();
        for chunk in chunks(&ids) {
            let call = Marked {
                mark,
                ids: chunk.clone(),
            };
            match attempt(&host, call).await? {
                Ok(_) => result.succeeded.extend(chunk),
                Err(_) => {
                    result.partial = true;
                    result.failed.extend(chunk);
                }
            }
        }
        typed(&result)
    }

    /// The push configuration and the account's registered browsers.
    #[wasm_bindgen(js_name = pushConfig, unchecked_return_type = "PushConfig")]
    pub async fn push_config(host: GatewayHost) -> Result<JsValue, JsValue> {
        typed(&run(&host, Config).await?)
    }

    /// Register one browser push subscription.
    #[wasm_bindgen(js_name = pushRegister, unchecked_return_type = "PushRegistered")]
    pub async fn push_register(
        host: GatewayHost,
        #[wasm_bindgen(unchecked_param_type = "BrowserPushSubscription")] subscription: JsValue,
    ) -> Result<JsValue, JsValue> {
        let subscription: BrowserPushSubscription =
            tsify::serde_wasm_bindgen::from_value(subscription).map_err(unreadable)?;
        typed(&run(&host, Register { subscription }).await?)
    }

    /// Revoke one push subscription by id or endpoint, as the current
    /// session or, after a sign-out, as the `ended` session that owned it.
    #[wasm_bindgen(js_name = pushRevoke)]
    pub async fn push_revoke(
        host: GatewayHost,
        subscription_id: Option<String>,
        endpoint: Option<String>,
        ended: Option<EndedSession>,
    ) -> Result<bool, JsValue> {
        let call = Revoke {
            subscription_id,
            endpoint,
        };
        attempt_as(&host, call, ended.as_ref().map(EndedSession::credentials))
            .await?
            .map_err(|refusal| rejection(&refusal))
    }
}

#[cfg(test)]
#[path = "notifications_tests.rs"]
mod tests;
