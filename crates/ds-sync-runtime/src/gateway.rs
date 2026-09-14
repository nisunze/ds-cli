//! The gateway seam: one authenticated JSON call on one declared route.
//!
//! The loop never sees a URL, a bearer or a response body it did not ask for.
//! A host supplies the door that fits its identity model: the desktop shell
//! attaches the session credential the webview handed it ([`BearerGateway`]);
//! the `ds` host goes through its closed native client; a host with neither
//! answers every call with a typed refusal ([`UnavailableGateway`]) and opens
//! nothing.

use serde_json::Value;

use crate::routes::SyncRoute;

/// One authenticated JSON call on one declared route, answering the `data`
/// envelope. Implementations cannot choose a path, a method or a URL.
pub trait Gateway {
    fn post(&self, route: SyncRoute, body: &Value) -> Result<Value, String>;
}

/// A gateway this host cannot reach. Every call is the same typed refusal,
/// naming why; no socket is opened and no credential is looked for.
#[derive(Clone, Debug)]
pub struct UnavailableGateway {
    pub reason: String,
}

impl Gateway for UnavailableGateway {
    fn post(&self, _route: SyncRoute, _body: &Value) -> Result<Value, String> {
        Err(self.reason.clone())
    }
}

/// A validated gateway origin. A credential is only ever attached to this.
pub fn validated_origin(raw: &str) -> Result<String, String> {
    let url = url::Url::parse(raw.trim()).map_err(|_| "The gateway origin is unusable")?;
    if url.scheme() != "https"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.host_str().is_none_or(str::is_empty)
        || url.path() != "/"
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err("The gateway origin is unusable".to_string());
    }
    Ok(url.origin().ascii_serialization())
}

/// The desktop shell's door: a session bearer the signed-in webview supplied,
/// attached to a pinned origin and nothing else. Never written to disk, never
/// logged, never echoed back.
pub struct BearerGateway {
    origin: String,
    bearer: String,
}

impl BearerGateway {
    pub fn new(origin: String, bearer: String) -> Result<Self, String> {
        let origin = validated_origin(&origin)?;
        if bearer.is_empty() || bearer.bytes().any(|b| b.is_ascii_control()) {
            return Err("The gateway credential is unusable".into());
        }
        Ok(BearerGateway { origin, bearer })
    }

    pub fn origin(&self) -> &str {
        &self.origin
    }
}

impl Gateway for BearerGateway {
    fn post(&self, route: SyncRoute, body: &Value) -> Result<Value, String> {
        let route = route.declared();
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .max_redirects(0)
            .https_only(true)
            .timeout_global(Some(std::time::Duration::from_secs(route.timeout_s)))
            .build()
            .into();
        let encoded =
            serde_json::to_vec(body).map_err(|_| "the request could not be encoded".to_string())?;
        let response = agent
            .post(format!("{}{}", self.origin, route.path))
            .header("Authorization", &format!("Bearer {}", self.bearer))
            .header("Content-Type", "application/json")
            .send(&encoded[..])
            .map_err(|error| match error {
                // A status is a fact; a body may echo a credential, so it never
                // reaches a message.
                ureq::Error::StatusCode(status) => format!("the gateway answered {status}"),
                _ => "the gateway could not be reached".to_string(),
            })?;
        let text = response
            .into_body()
            .with_config()
            .limit(route.response_cap_bytes)
            .read_to_string()
            .map_err(|_| "the gateway answer could not be read".to_string())?;
        if text.len() as u64 > route.response_cap_bytes {
            return Err("the gateway answer exceeds its declared ceiling".to_string());
        }
        let value: Value = serde_json::from_str(&text)
            .map_err(|_| "the gateway answer is not the declared envelope".to_string())?;
        value
            .get("data")
            .cloned()
            .ok_or_else(|| "the gateway answer carries no data".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_gateway_origin_is_pinned_before_a_credential_is_attached() {
        assert!(validated_origin("https://gateway.example.com").is_ok());
        for bad in [
            "http://gateway.example.com",
            "https://user:pass@gateway.example.com",
            "https://gateway.example.com/api/v1",
            "https://gateway.example.com?query=1",
            "https://gateway.example.com#fragment",
            "not a url",
        ] {
            assert!(validated_origin(bad).is_err(), "{bad} must be refused");
        }
    }

    #[test]
    fn an_unavailable_gateway_refuses_every_route_with_its_reason() {
        let gateway = UnavailableGateway {
            reason: "no native gateway adapter".into(),
        };
        for route in [
            SyncRoute::ComputeArtifacts,
            SyncRoute::WorkOpen,
            SyncRoute::WorkPublish,
        ] {
            assert_eq!(
                gateway.post(route, &serde_json::json!({})),
                Err("no native gateway adapter".to_string())
            );
        }
    }
}
