//! One captured native gateway effect, produced only by a closed typed call.
//!
//! Domain validation and request body builders remain in their existing owners.
//! This module does not refresh, retry, classify service replies or read a store.

use ds_gateway_client::Operation;

use crate::transport::SecretRequestBody;

mod sealed {
    pub trait Sealed {}
}

/// The mechanical distinction between an ordinary GET and an explicit byte send.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GatewaySend {
    Call,
    Bytes,
}

/// The messaging fixture build retains its existing explicit native origin.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GatewayOrigin {
    Profile,
    Messaging,
}

/// Privileged host input. Private fields prevent a caller choosing a route,
/// credential or arbitrary header; only the sealed typed-call implementations
/// below can create a request. Deliberately neither Debug nor Serialize.
pub struct CapturedGatewayRequest<'a> {
    origin: &'a str,
    path: String,
    url: String,
    method: &'static str,
    operation: Option<Operation>,
    client_id: &'static str,
    api_key: &'a str,
    bearer: &'a str,
    email: &'a str,
    content_type: &'static str,
    accept: &'static str,
    response_limit: usize,
    timeout_seconds: u64,
    send: GatewaySend,
    origin_kind: GatewayOrigin,
    processing_lane: Option<&'static str>,
    body: SecretRequestBody,
}

impl CapturedGatewayRequest<'_> {
    pub fn gateway_origin(&self) -> &str {
        self.origin
    }
    pub fn path(&self) -> &str {
        &self.path
    }
    pub fn url(&self) -> &str {
        &self.url
    }
    pub fn method(&self) -> &'static str {
        self.method
    }
    pub fn operation(&self) -> Option<Operation> {
        self.operation
    }
    pub fn client_id(&self) -> &'static str {
        self.client_id
    }
    pub fn gateway_api_key(&self) -> &str {
        self.api_key
    }
    pub fn bearer_token(&self) -> &str {
        self.bearer
    }
    pub fn canonical_email(&self) -> &str {
        self.email
    }
    pub fn content_type(&self) -> &'static str {
        self.content_type
    }
    pub fn accept(&self) -> &'static str {
        self.accept
    }
    pub fn response_limit(&self) -> usize {
        self.response_limit
    }
    pub fn timeout_seconds(&self) -> u64 {
        self.timeout_seconds
    }
    pub fn send(&self) -> GatewaySend {
        self.send
    }
    pub fn origin_kind(&self) -> GatewayOrigin {
        self.origin_kind
    }
    pub fn processing_lane(&self) -> Option<&'static str> {
        self.processing_lane
    }
    pub fn body(&self) -> &[u8] {
        self.body.as_bytes()
    }
}

/// Implemented only for the reviewed native typed calls, never for caller JSON.
pub trait GatewayCall: sealed::Sealed {
    fn capture_gateway(&self) -> CapturedGatewayRequest<'_>;
}

macro_rules! capture_operation {
    ($call:ident; (fixed $call_type:ty)) => {
        Some(<$call_type>::GATEWAY_OPERATION)
    };
    ($call:ident; (selected)) => {
        Some($call.generated_operation())
    };
    ($call:ident; (optional)) => {
        $call.generated_operation()
    };
    ($call:ident; (absent)) => {
        None
    };
}

macro_rules! capture_url {
    ($call:ident; relative; $path:ident) => {
        format!("{}{}", $call.gateway_origin(), $path)
    };
    ($call:ident; declared; $path:ident) => {
        $call.url()
    };
}

macro_rules! capture_gateway_call {
    ($call:ident; $route:tt; $body:expr, $send:expr, $timeout:expr, $accept:expr, $content_type:expr, $url_mode:ident, $origin_kind:expr, $lane:expr) => {{
        let method = $call.method();
        let operation = capture_operation!($call; $route);
        let path: String = $call.path().into();
        CapturedGatewayRequest {
            origin: $call.gateway_origin(),
            url: capture_url!($call; $url_mode; path),
            path,
            method,
            operation,
            client_id: $call.client_id(),
            api_key: $call.gateway_api_key(),
            bearer: $call.bearer_token(),
            email: $call.canonical_email(),
            content_type: $content_type,
            accept: $accept,
            response_limit: $call.response_limit(),
            timeout_seconds: $timeout,
            send: $send,
            origin_kind: $origin_kind,
            processing_lane: $lane,
            body: $body,
        }
    }};
}

macro_rules! capture_kind {
    ($call:ident; directory; $route:tt) => {
        capture_gateway_call!($call; $route; SecretRequestBody::new(Vec::new()), GatewaySend::Call, 30,
            $call.content_type(), $call.content_type(), declared, GatewayOrigin::Profile, None)
    };
    ($call:ident; get; $route:tt) => {
        capture_gateway_call!($call; $route; SecretRequestBody::new(Vec::new()), GatewaySend::Call, $call.timeout_seconds(),
            $call.content_type(), $call.content_type(), declared, GatewayOrigin::Profile, None)
    };
    ($call:ident; sync; $route:tt) => {
        capture_gateway_call!($call; $route; $call.body(), GatewaySend::Bytes, $call.timeout_seconds(),
            "application/json", "application/json", relative, GatewayOrigin::Profile, None)
    };
    ($call:ident; forced; $route:tt) => {{
        // Legacy dynamic GET sends explicit empty bytes after building its body.
        // Preserve both that construction and its zeroizing drop behavior.
        let body = $call.body();
        let body = if $call.method() == "GET" { SecretRequestBody::new(Vec::new()) } else { body };
        capture_gateway_call!($call; $route; body, GatewaySend::Bytes, $call.timeout_seconds(),
            $call.content_type(), $call.content_type(), relative, GatewayOrigin::Profile, None)
    }};
    ($call:ident; ndjson; $route:tt) => {
        capture_gateway_call!($call; $route; $call.body(), GatewaySend::Bytes, $call.timeout_seconds(),
            $call.accept(), $call.content_type(), relative, GatewayOrigin::Profile, None)
    };
    ($call:ident; processing; $route:tt) => {
        capture_gateway_call!($call; $route; $call.body(), GatewaySend::Bytes, $call.timeout_seconds(),
            $call.content_type(), $call.content_type(), relative, GatewayOrigin::Profile, Some("standard"))
    };
    ($call:ident; messaging; $route:tt) => {
        capture_gateway_call!($call; $route; $call.body(), GatewaySend::Bytes, $call.timeout_seconds(),
            $call.content_type(), $call.content_type(), relative, GatewayOrigin::Messaging, None)
    };
    ($call:ident; bytes; $route:tt) => {
        capture_gateway_call!($call; $route; $call.body(), GatewaySend::Bytes, $call.timeout_seconds(),
            $call.content_type(), $call.content_type(), relative, GatewayOrigin::Profile, None)
    };
    ($call:ident; json_bytes; $route:tt) => {
        capture_gateway_call!($call; $route; $call.body(), GatewaySend::Bytes, $call.timeout_seconds(),
            "application/json", "application/json", relative, GatewayOrigin::Profile, None)
    };
}

// The same closed list emits declarations and mechanical native forwarding.
crate::__ds_gateway_calls!(capture);

#[cfg(test)]
#[path = "gateway_call_tests.rs"]
mod tests;
