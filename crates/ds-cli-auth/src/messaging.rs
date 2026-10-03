//! Protected, account-scoped messaging host. No saved project selection.
use super::{
    Client, Lane, NativeRefreshStore, NativeTransport, map_client, now, profile,
    require_restore_before_context, restored_device_session,
};
use ds_cli_contract::Failure;
use ds_client_core::messaging::Command;
use serde_json::Value;
pub fn execute(lane_value: &str, command: &Command) -> Result<Value, Failure> {
    let lane = Lane::parse(lane_value)?;
    if let Some(mut device) = restored_device_session(lane)? {
        return device.messaging(command).map_err(map);
    }
    let profile = profile::load(lane)?;
    let store = NativeRefreshStore::open()?;
    let mut client = Client::new(profile, NativeTransport, store);
    require_restore_before_context(&mut client)?;
    client.messaging(command, now()).map_err(map)
}
fn map(error: ds_client_core::ClientError) -> Failure {
    let message = error.to_string();
    match message.as_str() {
        "messaging_request_invalid" => Failure::invalid(
            "messaging_request_invalid",
            "Brain rejected the messaging request",
        )
        .remedy("read the command contract and use server-issued ids and cursors"),
        "messaging_idempotency_mismatch" => Failure::conflict(
            "messaging_idempotency_mismatch",
            "This key already names different message content",
        )
        .remedy("retry the exact original content, or choose a new key for a new message"),
        "messaging_not_permitted" => Failure::unauthorized(
            "messaging_not_permitted",
            "Brain refused this identity or conversation access",
        )
        .remedy("restore the intended account or ask the project administrator for access"),
        "messaging_not_found" => Failure::invalid(
            "messaging_not_found",
            "This conversation is not visible to the caller",
        )
        .remedy("list or resolve conversations again"),
        "messaging_conflict" => {
            Failure::conflict("messaging_conflict", "Brain rejected the state transition")
                .remedy("read the current conversation and retry with its server-issued cursor")
        }
        "messaging_paused" => {
            Failure::unavailable("messaging_paused", "Messaging is paused by the operator")
                .remedy("wait for the operator to resume messaging")
        }
        "messaging_not_configured" => Failure::unavailable(
            "messaging_not_configured",
            "Messaging is not configured on this lane",
        )
        .remedy("use a lane with configured messaging"),
        "messaging_route_unavailable" => Failure::unavailable(
            "messaging_route_unavailable",
            "This gateway does not publish the messaging route",
        )
        .remedy("install the matching gateway and brain routes"),
        "messaging_rate_limited" => {
            Failure::unavailable("messaging_rate_limited", "Brain rate limited the caller")
                .remedy("wait before retrying the same request and idempotency key")
        }
        "messaging_response_unreadable" => Failure::unavailable(
            "messaging_response_unreadable",
            "Brain returned an invalid messaging representation",
        )
        .remedy("update ds and brain to the same wire contract"),
        "messaging_unavailable" => {
            Failure::unavailable("messaging_unavailable", "Brain could not serve messaging")
                .remedy("retry later with the same idempotency key")
        }
        _ => map_client(error),
    }
}

// A test build has one fixed loopback peer, selected at compilation. No
// runtime URL, bearer or transport override exists in the shipped executable.
#[cfg(ds_messaging_emulator)]
#[cfg(not(debug_assertions))]
compile_error!("the messaging emulator transport may only be compiled in debug builds");

#[cfg(ds_messaging_emulator)]
pub(crate) fn emulator_origin() -> &'static str {
    "http://127.0.0.1:18781"
}
