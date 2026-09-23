//! The gateway seam: one authenticated JSON call on one declared route.
//!
//! The loop never sees a URL, a bearer or a response body it did not ask for.
//! A host supplies the door that fits its identity model: the desktop shell
//! attaches the session credential the webview handed it ([`BearerGateway`]);
//! the `ds` host goes through its closed native client; a host with neither
//! answers every call with a typed refusal ([`UnavailableGateway`]) and opens
//! nothing.

use std::fmt;

use serde_json::Value;

use crate::routes::SyncRoute;

/// Why one gateway call answered no `data`.
///
/// `status` is the service's HTTP verdict when it gave one, so a caller can
/// tell "the record refused this" from "the record was not reached" — the
/// difference between a publication that is settled and one that is held.
/// `None` is every failure that never reached a verdict: offline, no route,
/// an unreadable answer. `detail` is safe to show and never carries a
/// credential.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GatewayError {
    pub status: Option<u16>,
    /// The authority's closed error code (`error.code`), when it named one.
    pub code: Option<String>,
    pub detail: String,
}

impl GatewayError {
    pub fn refused(status: u16, code: Option<String>, detail: impl Into<String>) -> Self {
        GatewayError {
            status: Some(status),
            code,
            detail: detail.into(),
        }
    }
}

impl From<String> for GatewayError {
    fn from(detail: String) -> Self {
        GatewayError {
            status: None,
            code: None,
            detail,
        }
    }
}

impl From<&str> for GatewayError {
    fn from(detail: &str) -> Self {
        detail.to_string().into()
    }
}

impl From<GatewayError> for String {
    fn from(error: GatewayError) -> Self {
        error.detail
    }
}

impl fmt::Display for GatewayError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.detail)
    }
}

/// One authenticated JSON call on one declared route, answering the `data`
/// envelope. Implementations cannot choose a path, a method or a URL.
pub trait Gateway {
    fn post(&self, route: SyncRoute, body: &Value) -> Result<Value, GatewayError>;
}

/// A gateway this host cannot reach. Every call is the same typed refusal,
/// naming why; no socket is opened and no credential is looked for.
#[derive(Clone, Debug)]
pub struct UnavailableGateway {
    pub reason: String,
}

impl Gateway for UnavailableGateway {
    fn post(&self, _route: SyncRoute, _body: &Value) -> Result<Value, GatewayError> {
        Err(self.reason.clone().into())
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
    fn post(&self, route: SyncRoute, body: &Value) -> Result<Value, GatewayError> {
        let route = route.declared();
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .max_redirects(0)
            .https_only(true)
            .http_status_as_error(false)
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
            .map_err(|_| "the gateway could not be reached".to_string())?;
        let status = response.status().as_u16();
        let text = response
            .into_body()
            .with_config()
            .limit(route.response_cap_bytes)
            .read_to_string();
        if status >= 400 {
            let body = text.ok();
            return Err(GatewayError::refused(
                status,
                refusal_field(body.as_deref(), "code"),
                refusal_detail(status, body.as_deref()),
            ));
        }
        let text = text.map_err(|_| "the gateway answer could not be read".to_string())?;
        if text.len() as u64 > route.response_cap_bytes {
            return Err("the gateway answer exceeds its declared ceiling".into());
        }
        let value: Value = serde_json::from_str(&text)
            .map_err(|_| "the gateway answer is not the declared envelope".to_string())?;
        value
            .get("data")
            .cloned()
            .ok_or_else(|| "the gateway answer carries no data".into())
    }
}

/// A refusal in the authority's own words when the answer is ds-brain's error
/// envelope, and the bare status otherwise. Only `error.code` and
/// `error.message` are ever read — never an arbitrary body, which a proxy
/// could fill with anything — bounded and stripped of control characters.
fn refusal_detail(status: u16, body: Option<&str>) -> String {
    match (refusal_field(body, "code"), refusal_field(body, "message")) {
        (Some(code), Some(message)) => format!("the gateway answered {status} {code}: {message}"),
        (Some(code), None) => format!("the gateway answered {status} {code}"),
        (None, Some(message)) => format!("the gateway answered {status}: {message}"),
        (None, None) => format!("the gateway answered {status}"),
    }
}

/// One field of ds-brain's `error` envelope, bounded and printable.
fn refusal_field(body: Option<&str>, name: &str) -> Option<String> {
    body.and_then(|text| serde_json::from_str::<Value>(text).ok())
        .and_then(|value| value.get("error")?.get(name)?.as_str().map(str::to_string))
        .map(|text| {
            text.chars()
                .filter(|c| !c.is_control())
                .take(300)
                .collect::<String>()
        })
        .filter(|text| !text.trim().is_empty())
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
        for route in [SyncRoute::ComputeArtifacts] {
            assert_eq!(
                gateway.post(route, &serde_json::json!({})),
                Err(GatewayError {
                    status: None,
                    code: None,
                    detail: "no native gateway adapter".to_string()
                })
            );
        }
    }

    /// A refusal says who refused and why, in the authority's words, and
    /// nothing a proxy could smuggle through a body.
    #[test]
    fn a_refusal_carries_the_authoritys_code_and_message_and_nothing_else() {
        assert_eq!(
            refusal_detail(
                403,
                Some(r#"{"success":false,"error":{"code":"INSUFFICIENT_PERMISSIONS","message":"needs design.report"}}"#)
            ),
            "the gateway answered 403 INSUFFICIENT_PERMISSIONS: needs design.report"
        );
        assert_eq!(
            refusal_detail(502, Some("<html>Bearer abc.def.ghi</html>")),
            "the gateway answered 502"
        );
        assert_eq!(refusal_detail(409, None), "the gateway answered 409");
        assert_eq!(
            refusal_field(Some(r#"{"error":{"code":"WORK_GRANT_REQUIRED"}}"#), "code").as_deref(),
            Some("WORK_GRANT_REQUIRED")
        );
        let long = format!(
            r#"{{"error":{{"code":"CONFLICT","message":"{}"}}}}"#,
            "x".repeat(900)
        );
        let detail = refusal_detail(409, Some(&long));
        assert!(detail.len() < 360 && !detail.chars().any(char::is_control));
    }
}
