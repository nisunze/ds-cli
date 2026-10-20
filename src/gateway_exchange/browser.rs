//! The browser executor of gateway exchanges: every step of a typed call,
//! sent with reqwest's WebAssembly client over the page's own `fetch`.
//!
//! Svelte calls one typed export per operation; each domain declares its
//! exports beside its operations, and every export runs [`run`]. The page
//! passes a [`GatewayHost`] answering only what the page holds:
//! `credentials()` — the session's bearer and email, asked before every
//! request so a long exchange never sends a stale bearer (the gateway origin
//! and the platform key are constants of the build,
//! [`crate::gateway_target`]); `facts()` — its clock and its session,
//! offline and link state; `refused()` — told each refusal that followed a request,
//! with the SRE event when the policy decided one is worth posting.
//! Headers, the deadline, the response bound, pauses and refusals happen
//! here. A refusal rejects as an `Error` named `GatewayRefusal` (or
//! `SignedOutRequestError`) that carries its `code`.

use std::future::Future;
use std::pin::pin;

use futures_util::future::{select, Either};
use futures_util::StreamExt;
use gloo_timers::future::TimeoutFuture;
use js_sys::{Promise, Reflect};
use serde::{Deserialize, Serialize};
use tsify::Tsify;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;

use super::{
    offline_refusal, resume, session_changed_refusal, start, Exchange, Facts, Failure, Method,
    Observed, Refusal, Report, Request, Step,
};

/// The client this executor speaks for, on every request.
const APP_ID: &str = "ds-web";
const PROCESSING_LANE_HEADER: &str = "X-DS-Processing-Lane";
const PROCESSING_LANE: &str = "fast";

#[wasm_bindgen(typescript_custom_section)]
const GATEWAY_HOST: &'static str = r#"
/** What the page answers for a gateway call; every decision is the Kernel's. */
export interface GatewayHost {
    /** The session's bearer and email, for the next request. A rejection ends the call with it. */
    credentials(): Promise<GatewayCredentials>;
    /** The page's clock and its session, offline and link state right now. */
    facts(): GatewayFacts;
    /** One refusal that followed a request; `report` is the SRE event to post, when the policy decided one is worth it. */
    refused(refused: GatewayRefused): void;
}

/** How a gateway call rejects. */
export interface GatewayRefusal extends Error {
    code: string;
    status?: number;
    transient: boolean;
    retryable: boolean;
    signedOut: boolean;
    outcomeUnknown: boolean;
    details?: unknown;
}
"#;

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(typescript_type = "GatewayHost")]
    pub type GatewayHost;

    #[wasm_bindgen(method, catch, js_name = credentials)]
    fn ask_credentials(this: &GatewayHost) -> Result<JsValue, JsValue>;

    #[wasm_bindgen(method, catch, js_name = facts)]
    fn ask_facts(this: &GatewayHost) -> Result<JsValue, JsValue>;

    #[wasm_bindgen(method, catch, js_name = refused)]
    fn tell_refused(this: &GatewayHost, refused: JsValue) -> Result<(), JsValue>;
}

/// What only the page holds: the session's bearer and email. The gateway
/// origin and the platform key are constants of this build
/// ([`crate::gateway_target`]).
#[derive(Deserialize, Tsify)]
pub struct GatewayCredentials {
    #[serde(default)]
    #[tsify(optional)]
    pub token: Option<String>,
    #[serde(default)]
    pub email: String,
}

/// Where this build's requests go: the deployed gateway it was compiled for,
/// or, on the local stack, the page's own origin, which proxies it.
fn gateway_origin() -> Result<String, JsValue> {
    if let Some(origin) = crate::gateway_target::GATEWAY_ORIGIN {
        return Ok(origin.to_owned());
    }
    Reflect::get(&js_sys::global(), &"location".into())
        .and_then(|location| Reflect::get(&location, &"origin".into()))
        .ok()
        .and_then(|origin| origin.as_string())
        .filter(|origin| !origin.is_empty())
        .ok_or_else(|| unreadable("the local stack's page origin"))
}

#[derive(Deserialize, Tsify)]
#[serde(deny_unknown_fields)]
pub struct GatewayFacts {
    pub now_ms: u64,
    pub has_session: bool,
    pub offline_mode: bool,
    /// Whether the platform reports any network link at all.
    pub link_up: bool,
    /// The signed-in account's stable id, empty when signed out; an answer
    /// that arrives for another account than the one that asked is refused.
    #[serde(default)]
    pub principal: String,
}

/// One refusal that followed a request, as the page's diagnostics read it.
#[derive(Serialize, Tsify)]
pub struct GatewayRefused {
    pub method: Method,
    pub path: String,
    pub request_id: String,
    pub latency_ms: u64,
    #[tsify(optional)]
    pub status: Option<u16>,
    pub code: String,
    pub message: String,
    #[tsify(optional)]
    pub technical: Option<String>,
    #[tsify(optional)]
    pub report: Option<Report>,
}

fn unreadable(error: impl std::fmt::Display) -> JsValue {
    js_sys::Error::new(&format!(
        "The gateway host answered an unreadable value: {error}"
    ))
    .into()
}

async fn credentials(host: &GatewayHost) -> Result<GatewayCredentials, JsValue> {
    let answer = JsFuture::from(Promise::resolve(&host.ask_credentials()?)).await?;
    tsify::serde_wasm_bindgen::from_value(answer).map_err(unreadable)
}

pub(crate) fn facts(host: &GatewayHost) -> Result<GatewayFacts, JsValue> {
    tsify::serde_wasm_bindgen::from_value(host.ask_facts()?).map_err(unreadable)
}

impl From<&GatewayFacts> for Facts {
    fn from(facts: &GatewayFacts) -> Self {
        Self {
            now_ms: facts.now_ms,
            has_session: facts.has_session,
            offline_mode: facts.offline_mode,
        }
    }
}

/// A typed answer for Svelte: the value as the plain object its generated
/// TypeScript interface describes. Maps, including free-form JSON members,
/// become plain objects too.
pub(crate) fn typed<T: Serialize>(value: &T) -> Result<JsValue, JsValue> {
    let plain = tsify::serde_wasm_bindgen::Serializer::new().serialize_maps_as_objects(true);
    value
        .serialize(&plain)
        .map_err(|error| js_sys::Error::new(&error.to_string()).into())
}

/// The refusal as the `Error` the page catches.
pub(crate) fn rejection(refusal: &Refusal) -> JsValue {
    let error = js_sys::Error::new(&refusal.message);
    error.set_name(if refusal.signed_out {
        "SignedOutRequestError"
    } else {
        "GatewayRefusal"
    });
    let set = |key: &str, value: JsValue| {
        let _ = Reflect::set(&error, &JsValue::from_str(key), &value);
    };
    set("code", JsValue::from_str(&refusal.code));
    if let Some(status) = refusal.status {
        set("status", JsValue::from(status));
    }
    set("transient", JsValue::from(refusal.transient));
    set("retryable", JsValue::from(refusal.retryable));
    set("signedOut", JsValue::from(refusal.signed_out));
    set("outcomeUnknown", JsValue::from(refusal.outcome_unknown));
    if let Some(details) = &refusal.details {
        if let Ok(details) = typed(details) {
            set("details", details);
        }
    }
    error.into()
}

/// Run `work` until it completes or its deadline passes (`None`). The loser
/// is dropped, which aborts an unfinished fetch.
async fn within<T>(work: impl Future<Output = T>, deadline_ms: u64) -> Option<T> {
    if deadline_ms == 0 {
        return Some(work.await);
    }
    let deadline = TimeoutFuture::new(u32::try_from(deadline_ms).unwrap_or(u32::MAX));
    match select(pin!(work), deadline).await {
        Either::Left((value, _)) => Some(value),
        Either::Right(_) => None,
    }
}

enum Read {
    Answer {
        status: u16,
        content_type: Option<String>,
        retry_after: Option<String>,
        bytes: Vec<u8>,
    },
    Oversized,
    /// The fetch itself failed (`true`) or the body broke off (`false`).
    Failed(bool),
}

async fn exchange(builder: reqwest::RequestBuilder, cap: u64) -> Read {
    let response = match builder.send().await {
        Ok(response) => response,
        Err(_) => return Read::Failed(true),
    };
    let header = |name: reqwest::header::HeaderName| {
        response
            .headers()
            .get(name)
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned)
    };
    let status = response.status().as_u16();
    let content_type = header(reqwest::header::CONTENT_TYPE);
    let retry_after = header(reqwest::header::RETRY_AFTER);
    if response.content_length().is_some_and(|length| length > cap) {
        return Read::Oversized;
    }
    let mut stream = response.bytes_stream();
    let mut bytes = Vec::new();
    while let Some(chunk) = stream.next().await {
        match chunk {
            Ok(chunk) if (bytes.len() + chunk.len()) as u64 > cap => return Read::Oversized,
            Ok(chunk) => bytes.extend_from_slice(&chunk),
            Err(_) => return Read::Failed(false),
        }
    }
    Read::Answer {
        status,
        content_type,
        retry_after,
        bytes,
    }
}

fn http_method(method: Method) -> reqwest::Method {
    match method {
        Method::Get => reqwest::Method::GET,
        Method::Post => reqwest::Method::POST,
        Method::Put => reqwest::Method::PUT,
        Method::Patch => reqwest::Method::PATCH,
        Method::Delete => reqwest::Method::DELETE,
    }
}

fn builder(
    client: &reqwest::Client,
    request: &Request,
    credentials: &GatewayCredentials,
    request_id: &str,
) -> Result<reqwest::RequestBuilder, JsValue> {
    let mut url = url::Url::parse(&gateway_origin()?)
        .and_then(|origin| origin.join(&request.path))
        .map_err(|_| unreadable("an invalid gateway origin"))?;
    if !request.query.is_empty() {
        url.query_pairs_mut().extend_pairs(&request.query);
    }
    let mut builder = client
        .request(http_method(request.method), url)
        .header("Content-Type", "application/json");
    if request.minimal_headers {
        if let Some(key) = crate::gateway_target::PLATFORM_API_KEY {
            builder = builder.header("x-api-key", key);
        }
        if let Some(token) = credentials
            .token
            .as_deref()
            .filter(|token| !token.is_empty())
        {
            builder = builder.header("Authorization", format!("Bearer {token}"));
        }
        if let Some(body) = &request.body {
            builder = builder.body(serde_json::to_vec(body).map_err(unreadable)?);
        }
        return Ok(builder);
    }
    builder = builder
        .header("X-App-Id", APP_ID)
        .header("X-Request-Id", request_id)
        .header("X-DS-Action-Id", request_id);
    if request.lane_header {
        builder = builder.header(PROCESSING_LANE_HEADER, PROCESSING_LANE);
    }
    let email = credentials.email.trim();
    if !email.is_empty() {
        builder = builder.header("X-User-Email", email);
    }
    if let Some(key) = crate::gateway_target::PLATFORM_API_KEY {
        builder = builder.header("x-api-key", key);
    }
    if let Some(token) = credentials
        .token
        .as_deref()
        .filter(|token| !token.is_empty())
    {
        let bearer = format!("Bearer {token}");
        builder = builder
            .header("Authorization", &bearer)
            .header("X-Forwarded-Authorization", bearer);
    }
    if let Some(body) = &request.body {
        builder = builder.body(serde_json::to_vec(body).map_err(unreadable)?);
    }
    Ok(builder)
}

/// Send one request and observe its answer, or why none arrived.
async fn send(
    client: &reqwest::Client,
    request: &Request,
    credentials: &GatewayCredentials,
    request_id: &str,
    host: &GatewayHost,
) -> Result<(Observed, Vec<u8>), JsValue> {
    let pending = exchange(
        builder(client, request, credentials, request_id)?,
        request.response_cap_bytes,
    );
    let failure = |cause, message: &str| {
        (
            Observed::Failure {
                cause,
                message: Some(message.into()),
            },
            Vec::new(),
        )
    };
    Ok(match within(pending, request.timeout_ms).await {
        None => failure(Failure::Timeout, ""),
        Some(Read::Oversized) => failure(Failure::Oversized, ""),
        Some(Read::Failed(before_answer)) => {
            let cause = if facts(host)?.link_up {
                Failure::Network
            } else {
                Failure::Offline
            };
            failure(
                cause,
                if before_answer {
                    "Failed to fetch"
                } else {
                    "The connection closed before the answer was complete"
                },
            )
        }
        Some(Read::Answer {
            status,
            content_type,
            retry_after,
            bytes,
        }) => (
            Observed::Response {
                status,
                content_type,
                retry_after,
            },
            bytes,
        ),
    })
}

struct Sent {
    method: Method,
    path: String,
    request_id: String,
    latency_ms: u64,
}

/// Tell the host about a refusal that followed a request.
fn tell(host: &GatewayHost, refusal: &Refusal, sent: Option<Sent>) {
    let Some(sent) = sent.filter(|_| !refusal.signed_out) else {
        return;
    };
    let told = GatewayRefused {
        method: sent.method,
        path: sent.path,
        request_id: sent.request_id,
        latency_ms: sent.latency_ms,
        status: refusal.status,
        code: refusal.code.clone(),
        message: refusal.message.clone(),
        technical: refusal.technical.clone(),
        report: refusal.report.clone(),
    };
    if let Ok(told) = typed(&told) {
        let _ = host.tell_refused(told);
    }
}

/// Run one typed call to its end through the page's `fetch`, answering its
/// typed outcome; a flow that branches on the refusal reads it here. The
/// host is told of a refusal exactly as [`run`] tells it. Only a host or
/// executor failure is the outer `Err`.
pub(crate) async fn attempt<C: Exchange>(
    host: &GatewayHost,
    call: C,
) -> Result<Result<C::Output, Refusal>, JsValue> {
    let client = reqwest::Client::new();
    // The account that asked; an answer is only ever handed back to it.
    let principal = facts(host)?.principal;
    let mut sent = None;
    let mut step = start(call);
    loop {
        step = match step {
            Step::Done(value) => return Ok(Ok(value)),
            Step::Refused(refusal) => {
                tell(host, &refusal, sent);
                return Ok(Err(refusal));
            }
            Step::Wait {
                delay_ms,
                jitter_ms,
                continuation,
            } => {
                let delay = delay_ms + (js_sys::Math::random() * jitter_ms as f64) as u64;
                TimeoutFuture::new(u32::try_from(delay).unwrap_or(u32::MAX)).await;
                resume(continuation, Observed::Waited, &[], &(&facts(host)?).into())
            }
            Step::Fetch {
                request,
                continuation,
            } => {
                let before = facts(host)?;
                if before.offline_mode {
                    return Ok(Err(offline_refusal()));
                }
                if before.principal != principal {
                    return Ok(Err(session_changed_refusal()));
                }
                let credentials = credentials(host).await?;
                let request_id = uuid::Uuid::new_v4().to_string();
                let started = js_sys::Date::now();
                let (observed, bytes) =
                    send(&client, &request, &credentials, &request_id, host).await?;
                sent = Some(Sent {
                    method: request.method,
                    path: request.path.clone(),
                    request_id,
                    latency_ms: (js_sys::Date::now() - started).max(0.0) as u64,
                });
                let after = facts(host)?;
                if after.principal != principal {
                    return Ok(Err(session_changed_refusal()));
                }
                resume(continuation, observed, &bytes, &(&after).into())
            }
        };
    }
}

/// Run one typed call to its end; a refusal rejects as the page's `Error`.
pub(crate) async fn run<C: Exchange>(host: &GatewayHost, call: C) -> Result<C::Output, JsValue> {
    attempt(host, call)
        .await?
        .map_err(|refusal| rejection(&refusal))
}
