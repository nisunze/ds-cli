//! Mechanical native execution of an already captured closed gateway request.
//! Included as a child of transport.rs; it shares that adapter's bounds,
//! correlation, local-Go selection and network-cause classification.

#[cfg(ds_messaging_emulator)]
use ds_client_core::GatewayOrigin;
use ds_client_core::{CapturedGatewayRequest, GatewaySend};
use zeroize::Zeroizing;

use super::{CONNECT_TIMEOUT, bounded, classify, correlation_headers, local_go};
use ds_client_core::{TransportError, TransportResponse};
use std::time::Duration;

fn headers<'a>(
    call: &'a CapturedGatewayRequest<'_>,
    bearer: &'a str,
    request_id: &'a str,
    action_id: &'a str,
) -> [(&'static str, &'a str); 9] {
    [
        ("Accept", call.accept()),
        ("Content-Type", call.content_type()),
        ("X-App-Id", call.client_id()),
        ("X-Request-Id", request_id),
        ("X-DS-Action-Id", action_id),
        ("X-User-Email", call.canonical_email()),
        ("x-api-key", call.gateway_api_key()),
        ("Authorization", bearer),
        ("X-Forwarded-Authorization", bearer),
    ]
}

pub(super) fn send(
    _transport: &mut super::NativeTransport,
    call: CapturedGatewayRequest<'_>,
) -> Result<TransportResponse, TransportError> {
    let (request_id, action_id) = correlation_headers();
    let bearer = Zeroizing::new(format!("Bearer {}", call.bearer_token()));
    #[cfg(ds_messaging_emulator)]
    let url = if call.origin_kind() == GatewayOrigin::Messaging {
        format!("{}{}", crate::messaging::emulator_origin(), call.path())
    } else {
        local_go::api_url(call.gateway_origin(), call.url())?
    };
    #[cfg(not(ds_messaging_emulator))]
    let url = local_go::api_url(call.gateway_origin(), call.url())?;

    if call.send() == GatewaySend::Call {
        let mut request = local_go::api_agent()?.get(url);
        for (name, value) in headers(&call, bearer.as_str(), &request_id, &action_id) {
            request = request.header(name, value);
        }
        let response = request
            .config()
            .max_redirects(0)
            .http_status_as_error(false)
            .timeout_connect(Some(CONNECT_TIMEOUT))
            .timeout_global(Some(Duration::from_secs(call.timeout_seconds())))
            .build()
            .call()
            .map_err(classify)?;
        return bounded(response, call.response_limit());
    }

    let agent = local_go::api_agent()?;
    let request = match call.method() {
        "GET" => agent.get(url).force_send_body(),
        "POST" => agent.post(url),
        "PUT" => agent.put(url),
        "PATCH" => agent.patch(url),
        _ => return Err(TransportError::Unreachable),
    };
    let mut request = request;
    for (name, value) in headers(&call, bearer.as_str(), &request_id, &action_id) {
        request = request.header(name, value);
    }
    if let Some(lane) = call.processing_lane() {
        request = request.header("X-DS-Processing-Lane", lane);
    }
    let response = request
        .config()
        .max_redirects(0)
        .http_status_as_error(false)
        .timeout_connect(Some(CONNECT_TIMEOUT))
        .timeout_global(Some(Duration::from_secs(call.timeout_seconds())))
        .build()
        .send(call.body())
        .map_err(classify)?;
    bounded(response, call.response_limit())
}

#[cfg(test)]
#[path = "gateway_transport_tests.rs"]
mod tests;
