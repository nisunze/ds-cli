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

/// One messaging command.
#[derive(Clone, Debug, PartialEq)]
pub struct Messaging {
    pub endpoint: MessagingEndpoint,
    pub payload: Map<String, Value>,
    pub timeout_ms: Option<u64>,
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

/// The typed browser export Svelte calls; it runs the shared executor.
#[cfg(all(feature = "wasm", target_arch = "wasm32"))]
mod exports {
    use super::{Messaging, MessagingEndpoint};
    use crate::gateway_exchange::browser::{run, typed, GatewayHost};
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
        typed(&run(&host, call).await?)
    }
}

#[cfg(test)]
#[path = "messaging_tests.rs"]
mod tests;
