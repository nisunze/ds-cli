//! Messaging: one command of Brain's messaging service or of its attachment
//! sessions, admitted only when Brain serves its action. Brain binds the
//! caller to the bearer, applies its own rate limits and answers in the wire
//! contract the request names; the page's messaging decoders read the
//! envelope as served. Messaging answers its own CORS preflight, so these
//! requests carry only the headers it admits.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use super::{Admitted, Exchange, Plan, Refusal, Request};

/// The wire contract this client decodes.
pub const WIRE_CONTRACT: &str = "messages-v1";
const DEFAULT_TIMEOUT_MS: u64 = 30_000;
const MAX_TIMEOUT_MS: u64 = 120_000;
/// Brain's own messaging response bound.
const RESPONSE_CAP: u64 = 2 * 1024 * 1024;
pub const INPUT_INVALID: &str = "messaging_input_invalid";
pub const INVALID: &str = "messaging_answer_invalid";

/// Which messaging route a command belongs to.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), derive(tsify::Tsify))]
#[serde(rename_all = "snake_case")]
pub enum MessagingEndpoint {
    Commands,
    Attachments,
}

impl MessagingEndpoint {
    fn path(self) -> &'static str {
        match self {
            Self::Commands => "/api/v1/messaging",
            Self::Attachments => "/api/v1/messaging/attachments",
        }
    }

    /// The actions Brain serves on the route.
    pub fn actions(self) -> &'static [&'static str] {
        match self {
            Self::Commands => &[
                "config",
                "search_people",
                "search_project_people",
                "list_contacts",
                "list_conversations",
                "resolve_direct",
                "resolve_notifications",
                "resolve_project",
                "list_messages",
                "list_members",
                "send_message",
                "advance_read",
                "advance_delivered",
                "set_preferences",
                "register_installation",
                "revoke_installation",
                "list_installations",
            ],
            Self::Attachments => &["start", "finalize", "download"],
        }
    }
}

/// The export the page calls, and the exchange it runs, for
/// [`crate::capabilities::admit_export`].
pub const EXPORT: &str = "messagingCommand";
pub const EXCHANGE: &str = "messaging";

/// The capability one action of one route performs: every action Brain
/// serves names exactly one manifest row.
pub fn capability(endpoint: MessagingEndpoint, action: &str) -> Option<&'static str> {
    Some(match (endpoint, action) {
        (MessagingEndpoint::Commands, "config") => "messaging.config",
        (MessagingEndpoint::Commands, "search_people" | "search_project_people" | "list_contacts") => {
            "messaging.people"
        }
        (MessagingEndpoint::Commands, "list_conversations") => "messaging.conversations",
        (MessagingEndpoint::Commands, "resolve_direct") => "messaging.direct",
        (MessagingEndpoint::Commands, "resolve_project") => "messaging.project",
        (MessagingEndpoint::Commands, "resolve_notifications") => "messaging.personal",
        (MessagingEndpoint::Commands, "list_messages" | "list_members" | "advance_delivered") => {
            "messaging.read"
        }
        (MessagingEndpoint::Commands, "send_message") => "messaging.send",
        (MessagingEndpoint::Commands, "advance_read") => "messaging.mark-read",
        (MessagingEndpoint::Commands, "set_preferences") => "messaging.preferences.set",
        (MessagingEndpoint::Commands, "register_installation") => "messaging.installation.register",
        (MessagingEndpoint::Commands, "revoke_installation") => "messaging.installation.revoke",
        (MessagingEndpoint::Commands, "list_installations") => "messaging.installations",
        (MessagingEndpoint::Attachments, "start" | "finalize") => "messaging.attachment.upload",
        (MessagingEndpoint::Attachments, "download") => "messaging.read",
        _ => return None,
    })
}

/// One messaging command.
#[derive(Clone, Debug, PartialEq)]
pub struct Messaging {
    pub endpoint: MessagingEndpoint,
    pub payload: Map<String, Value>,
    pub timeout_ms: Option<u64>,
}

impl Messaging {
    /// Admit this command from the page: its capability's row must name
    /// [`EXPORT`] and be supported on this build's platform.
    pub fn admit_in_page(&self, platform: &str) -> Result<&'static str, Refusal> {
        let action = self
            .payload
            .get("action")
            .and_then(Value::as_str)
            .map(|action| action.trim().to_ascii_lowercase())
            .unwrap_or_default();
        let id = capability(self.endpoint, &action).ok_or_else(|| {
            Refusal::new(
                INPUT_INVALID,
                format!("`{action}` is not a messaging action of this route."),
            )
        })?;
        crate::capabilities::admit_export_on(platform, id, EXPORT, EXCHANGE)
            .map(|capability| capability.id)
            .map_err(|refused| Refusal::new(refused.code, refused.message))
    }
}

impl Exchange for Messaging {
    type Output = Value;

    fn plan(&self) -> Result<Plan, Refusal> {
        let action = self
            .payload
            .get("action")
            .and_then(Value::as_str)
            .map(|action| action.trim().to_ascii_lowercase())
            .unwrap_or_default();
        if !self.endpoint.actions().contains(&action.as_str()) {
            return Err(Refusal::new(
                INPUT_INVALID,
                format!("`{action}` is not a messaging action of this route."),
            ));
        }
        let timeout_ms = match self.timeout_ms {
            None => DEFAULT_TIMEOUT_MS,
            Some(ms) if (1..=MAX_TIMEOUT_MS).contains(&ms) => ms,
            Some(_) => {
                return Err(Refusal::new(
                    INPUT_INVALID,
                    "A messaging command waits at most 120 seconds.",
                ))
            }
        };
        let body = Value::Object(
            self.payload
                .iter()
                .filter(|(_, value)| !value.is_null())
                .map(|(key, value)| (key.clone(), value.clone()))
                .collect(),
        );
        let request = Request {
            query: vec![("wire_contract".into(), WIRE_CONTRACT.into())],
            ..Request::post(self.endpoint.path(), body)
        }
        .timeout_ms(timeout_ms)
        .response_cap(RESPONSE_CAP)
        .minimal_headers();
        Ok(request.into())
    }

    fn admit(self, _status: u16, body: Value) -> Result<Admitted<Self>, Refusal> {
        if !body.is_object() {
            return Err(Refusal::new(
                INVALID,
                "Messaging answered an invalid envelope.",
            ));
        }
        Ok(Admitted::Done(body))
    }
}

/// The account's messaging read cache as the page last kept it: the
/// conversation list and up to 24 timelines, keyed by conversation id.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), derive(tsify::Tsify))]
pub struct MessagingCache {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(
        all(feature = "wasm", target_arch = "wasm32"),
        tsify(optional, type = "unknown")
    )]
    pub conversations: Option<Value>,
    #[serde(default)]
    pub timelines: Vec<HeldTimeline>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), derive(tsify::Tsify))]
pub struct HeldTimeline {
    pub conversation_id: String,
    #[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), tsify(type = "unknown"))]
    pub timeline: Value,
}

/// The cache this device holds for the signed-in account, with no transport.
#[cfg(feature = "durable-store")]
pub async fn held_cache(edge: &impl super::retained::Edge) -> Result<MessagingCache, Refusal> {
    use crate::retained_reads::Kind;
    let conversations =
        super::retained::read::<Value>(edge, Kind::MessagingConversations, None, "").await?;
    let timelines = super::retained::list::<Value>(edge, Kind::MessagingTimeline, None).await?;
    Ok(MessagingCache {
        conversations: conversations.held.map(|held| held.value),
        timelines: timelines
            .rows
            .into_iter()
            .map(|(conversation_id, held)| HeldTimeline {
                conversation_id,
                timeline: held.value,
            })
            .collect(),
    })
}

/// Keep the page's cache: the conversation list, and exactly these timelines.
#[cfg(feature = "durable-store")]
pub async fn keep_cache(
    edge: &impl super::retained::Edge,
    cache: &MessagingCache,
) -> Result<(), Refusal> {
    use crate::retained_reads::Kind;
    let observed =
        super::retained::read::<Value>(edge, Kind::MessagingConversations, None, "").await?;
    let owner = observed.owner.as_deref();
    if let Some(conversations) = &cache.conversations {
        let kind = Kind::MessagingConversations;
        super::retained::write(edge, owner, kind, None, "", conversations, None).await?;
    }
    let rows: Vec<(String, Value)> = cache
        .timelines
        .iter()
        .map(|held| (held.conversation_id.clone(), held.timeline.clone()))
        .collect();
    super::retained::replace_set(edge, owner, Kind::MessagingTimeline, None, &rows).await
}

/// Forget the signed-in account's cache, on sign-out or an account switch.
#[cfg(feature = "durable-store")]
pub async fn forget_cache(edge: &impl super::retained::Edge) -> Result<(), Refusal> {
    use crate::retained_reads::Kind;
    let observed =
        super::retained::read::<Value>(edge, Kind::MessagingConversations, None, "").await?;
    let owner = observed.owner.as_deref();
    super::retained::forget(edge, owner, Kind::MessagingConversations, None, None).await?;
    super::retained::forget(edge, owner, Kind::MessagingTimeline, None, None).await
}

/// The typed browser export Svelte calls; it runs the shared executor.
#[cfg(all(feature = "wasm", target_arch = "wasm32"))]
mod exports {
    use super::{
        forget_cache, held_cache, keep_cache, Messaging, MessagingCache, MessagingEndpoint,
    };
    use crate::durable_store::port::StoreHost;
    use crate::gateway_exchange::browser::{rejection, run, typed, GatewayHost};
    use crate::gateway_exchange::retained::browser::BrowserEdge;
    use serde_json::{Map, Value};
    use wasm_bindgen::prelude::*;

    fn unreadable(error: impl std::fmt::Display) -> JsValue {
        js_sys::Error::new(&error.to_string()).into()
    }

    /// One messaging command; answers Brain's envelope as served.
    #[wasm_bindgen(js_name = messagingCommand, unchecked_return_type = "Record<string, unknown>")]
    pub async fn messaging_command(
        host: GatewayHost,
        #[wasm_bindgen(unchecked_param_type = "MessagingEndpoint")] endpoint: JsValue,
        #[wasm_bindgen(unchecked_param_type = "Record<string, unknown>")] payload: JsValue,
        timeout_ms: Option<u32>,
    ) -> Result<JsValue, JsValue> {
        let endpoint: MessagingEndpoint =
            tsify::serde_wasm_bindgen::from_value(endpoint).map_err(unreadable)?;
        let payload: Map<String, Value> =
            tsify::serde_wasm_bindgen::from_value(payload).map_err(unreadable)?;
        let call = Messaging {
            endpoint,
            payload,
            timeout_ms: timeout_ms.map(u64::from),
        };
        call.admit_in_page(crate::capabilities::PLATFORM)
            .map_err(|refusal| crate::gateway_exchange::browser::rejection(&refusal))?;
        typed(&run(&host, call).await?)
    }
    /// The signed-in account's messaging cache, at once and offline.
    #[wasm_bindgen(js_name = messagingCacheHeld, unchecked_return_type = "MessagingCache")]
    pub async fn messaging_cache_held(
        host: GatewayHost,
        store: StoreHost,
    ) -> Result<JsValue, JsValue> {
        let edge = BrowserEdge::new(host, store);
        typed(
            &held_cache(&edge)
                .await
                .map_err(|refusal| rejection(&refusal))?,
        )
    }

    /// Keep the page's messaging cache for the next offline open.
    #[wasm_bindgen(js_name = messagingCacheKeep)]
    pub async fn messaging_cache_keep(
        host: GatewayHost,
        store: StoreHost,
        #[wasm_bindgen(unchecked_param_type = "MessagingCache")] cache: JsValue,
    ) -> Result<(), JsValue> {
        let cache: MessagingCache = tsify::serde_wasm_bindgen::from_value(cache)
            .map_err(|error| js_sys::Error::new(&error.to_string()))?;
        let edge = BrowserEdge::new(host, store);
        keep_cache(&edge, &cache)
            .await
            .map_err(|refusal| rejection(&refusal))
    }

    /// Forget the signed-in account's messaging cache.
    #[wasm_bindgen(js_name = messagingCacheForget)]
    pub async fn messaging_cache_forget(
        host: GatewayHost,
        store: StoreHost,
    ) -> Result<(), JsValue> {
        let edge = BrowserEdge::new(host, store);
        forget_cache(&edge)
            .await
            .map_err(|refusal| rejection(&refusal))
    }
}

#[cfg(test)]
#[path = "messaging_tests.rs"]
mod tests;
