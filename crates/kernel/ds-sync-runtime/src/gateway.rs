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

    /// The session credential did not pass the gateway or the authority.
    pub fn credential_refused(&self) -> bool {
        self.status == Some(401)
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

/// A validated gateway origin. A credential is attached only to HTTPS or to
/// the local Desktop's exact loopback HTTP host during development.
pub fn validated_origin(raw: &str) -> Result<String, String> {
    let url = url::Url::parse(raw.trim()).map_err(|_| "The gateway origin is unusable")?;
    let local_http = url.scheme() == "http"
        && matches!(
            url.host(),
            Some(url::Host::Domain("localhost"))
                | Some(url::Host::Ipv4(std::net::Ipv4Addr::LOCALHOST))
                | Some(url::Host::Ipv6(std::net::Ipv6Addr::LOCALHOST))
        );
    if !(url.scheme() == "https" || local_http)
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

/// The desktop shell's door: the gateway API key and session bearer the
/// signed-in webview supplied, attached to a pinned origin and nothing else.
/// Neither credential is written to disk, logged or echoed back.
pub struct BearerGateway {
    origin: String,
    api_key: String,
    bearer: String,
}

impl BearerGateway {
    pub fn new(origin: String, api_key: String, bearer: String) -> Result<Self, String> {
        let origin = validated_origin(&origin)?;
        if api_key.len() > 256
            || !api_key.bytes().all(|b| b.is_ascii_graphic())
            || bearer.is_empty()
            || bearer.bytes().any(|b| b.is_ascii_control())
        {
            return Err("The gateway credential is unusable".into());
        }
        Ok(BearerGateway {
            origin,
            api_key,
            bearer,
        })
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
            .https_only(self.origin.starts_with("https://"))
            .http_status_as_error(false)
            .timeout_global(Some(std::time::Duration::from_secs(route.timeout_s)))
            .build()
            .into();
        let encoded =
            serde_json::to_vec(body).map_err(|_| "the request could not be encoded".to_string())?;
        let mut request = agent
            .post(format!("{}{}", self.origin, route.path))
            .header("Authorization", &format!("Bearer {}", self.bearer))
            .header("Content-Type", "application/json");
        // The API Gateway checks this header before it checks the bearer.
        // The local stack has no API key, so an empty key sends no header.
        if !self.api_key.is_empty() {
            request = request.header("x-api-key", &self.api_key);
        }
        let response = request
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
    use std::io::{BufRead, BufReader, Read, Write};
    use std::net::TcpListener;

    fn through_gateway(api_key: &str) -> (Result<Value, GatewayError>, String) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let worker = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(std::time::Duration::from_secs(3)))
                .unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut head = String::new();
            loop {
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                assert!(!line.is_empty(), "the request ended before its headers");
                head.push_str(&line);
                if line == "\r\n" {
                    break;
                }
            }
            // Consume the POST body before closing the socket. Closing with
            // unread request bytes can reset a valid response nondeterministically.
            let length = head
                .lines()
                .find_map(|line| {
                    let (name, value) = line.split_once(':')?;
                    name.eq_ignore_ascii_case("content-length")
                        .then(|| value.trim().parse::<usize>().unwrap())
                })
                .expect("JSON POST content length");
            assert!(length <= 1024);
            let mut request = vec![0; length];
            reader.read_exact(&mut request).unwrap();
            assert_eq!(
                serde_json::from_slice::<Value>(&request).unwrap(),
                serde_json::json!({"action":"list"})
            );
            let keyed = head
                .to_ascii_lowercase()
                .contains("\r\nx-api-key: gateway-key\r\n");
            let (status, body) = if keyed {
                ("200 OK", r#"{"data":{"head":"accepted"}}"#)
            } else {
                (
                    "401 Unauthorized",
                    r#"{"code":401,"message":"UNAUTHENTICATED: Method doesn't allow unregistered callers (callers without established identity). Please use API Key or other form of API consumer identity to call this API."}"#,
                )
            };
            write!(
                stream,
                "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            )
            .unwrap();
            head
        });
        let gateway = BearerGateway::new(origin, api_key.into(), "session-bearer".into()).unwrap();
        let answer = gateway.post(
            SyncRoute::ComputeArtifacts,
            &serde_json::json!({"action":"list"}),
        );
        (answer, worker.join().unwrap())
    }

    /// The production API gateway rejects unregistered callers before it
    /// checks their bearer. The desktop's sync door must carry both headers.
    #[test]
    fn the_desktop_sync_door_carries_the_gateway_api_key() {
        let (answer, head) = through_gateway("");
        let refusal = answer.unwrap_err();
        assert_eq!(refusal.status, Some(401));
        assert!(refusal.credential_refused());
        assert_eq!(refusal.detail, "the gateway answered 401");
        assert!(!head.to_ascii_lowercase().contains("x-api-key:"));

        let (answer, head) = through_gateway("gateway-key");
        assert_eq!(answer.unwrap(), serde_json::json!({"head":"accepted"}));
        let head = head.to_ascii_lowercase();
        assert!(head.starts_with("post /api/v1/compute_artifacts "));
        assert!(head.contains("\r\nauthorization: bearer session-bearer\r\n"));
        assert!(head.contains("\r\nx-api-key: gateway-key\r\n"));
    }

    #[test]
    fn an_invalid_gateway_api_key_is_refused_before_any_request() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        for key in [
            "key\r\nx-forged: 1".to_string(),
            "x".repeat(257),
            "é".into(),
        ] {
            assert_eq!(
                BearerGateway::new(origin.clone(), key, "session-bearer".into())
                    .err()
                    .as_deref(),
                Some("The gateway credential is unusable")
            );
        }
        assert_eq!(
            listener.accept().unwrap_err().kind(),
            std::io::ErrorKind::WouldBlock
        );
        assert!(BearerGateway::new(origin, "x".repeat(256), "session-bearer".into()).is_ok());
        assert!(!GatewayError::refused(403, None, "forbidden").credential_refused());
    }

    #[test]
    fn a_gateway_origin_is_pinned_before_a_credential_is_attached() {
        assert!(validated_origin("https://gateway.example.com").is_ok());
        for local in [
            "http://localhost:5173",
            "http://127.0.0.1:5173",
            "http://[::1]:5173",
        ] {
            assert_eq!(validated_origin(local).unwrap(), local);
        }
        for bad in [
            "http://gateway.example.com",
            "http://localhost.evil.example:5173",
            "http://127.0.0.2:5173",
            "http://localhost:5173/api",
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
                Some(
                    r#"{"success":false,"error":{"code":"INSUFFICIENT_PERMISSIONS","message":"needs design.report"}}"#
                )
            ),
            "the gateway answered 403 INSUFFICIENT_PERMISSIONS: needs design.report"
        );
        assert_eq!(
            refusal_detail(502, Some("<html>Bearer abc.def.ghi</html>")),
            "the gateway answered 502"
        );
        assert_eq!(refusal_detail(409, None), "the gateway answered 409");
        assert_eq!(
            refusal_field(
                Some(r#"{"error":{"code":"INSUFFICIENT_PERMISSIONS"}}"#),
                "code"
            )
            .as_deref(),
            Some("INSUFFICIENT_PERMISSIONS")
        );
        let long = format!(
            r#"{{"error":{{"code":"CONFLICT","message":"{}"}}}}"#,
            "x".repeat(900)
        );
        let detail = refusal_detail(409, Some(&long));
        assert!(detail.len() < 360 && !detail.chars().any(char::is_control));
    }
}
