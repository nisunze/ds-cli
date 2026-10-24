//! Why a storage object read ended without verified bytes, as the closed
//! cause the Sync Center keeps on the failed `download` receipt
//! (`failure_code`, with a bounded `detail`).
//!
//! The tokens are the ones the gateway transport uses
//! (`ds-cli-auth` `network_cause`: `deadline_exceeded`, `service_status`,
//! `connection_refused`, ...), so a status line reads the same whether the DS
//! gateway or the storage host failed. A read adds exactly two of its own, for
//! the verification it owes before a byte is placed: `digest_mismatch` and
//! `size_mismatch`. A failure of this machine's own disk, or a refused ticket,
//! carries no code: it is not the network and not the object.
//!
//! None of this changes a retry: the error stays unanswered (`status: None`),
//! so the loop treats it as it always did, as the moment, and the next pass
//! reads the object again.

use crate::gateway::GatewayError;

/// The downloaded bytes hash to something other than the head's `sha256`.
pub const DIGEST_MISMATCH: &str = "digest_mismatch";
/// The object is longer or shorter than the head's `size_bytes`.
pub const SIZE_MISMATCH: &str = "size_mismatch";

/// The statuses a retry can heal (`ds_client_core::weak_network::transient_status`).
const fn transient_status(status: u16) -> bool {
    matches!(status, 408 | 425 | 429 | 500..=599)
}

/// A request to the storage host that failed before a body arrived.
pub fn request_failure(error: &ureq::Error) -> GatewayError {
    use ureq::{Error, Timeout};
    let (code, sentence) = match error {
        Error::StatusCode(status) if transient_status(*status) => (
            "service_status",
            format!(
                "the storage host answered HTTP {status}: the service is failing or rate-limited"
            ),
        ),
        // A verdict (an expired or refused ticket, a missing object): the
        // sentence stays, with no transport code.
        Error::StatusCode(status) => {
            return GatewayError::from(format!("the storage host answered {status}"));
        }
        Error::Timeout(Timeout::Resolve) | Error::HostNotFound => (
            "name_unresolved",
            "this machine could not resolve the storage host name".to_string(),
        ),
        Error::Timeout(Timeout::Connect) => (
            "connect_timed_out",
            "the connection to the storage host did not open within the connect deadline"
                .to_string(),
        ),
        Error::Timeout(_) => (
            "deadline_exceeded",
            "the storage host took the request and did not answer within the call's deadline"
                .to_string(),
        ),
        Error::ConnectProxyFailed(_) | Error::InvalidProxyUrl => (
            "proxy_refused",
            "the HTTP proxy configured for this process refused the tunnel to the storage host"
                .to_string(),
        ),
        Error::ConnectionFailed => (
            "connection_refused",
            "the connection to the storage host was refused on this machine's network path"
                .to_string(),
        ),
        Error::Tls(_) | Error::TlsRequired | Error::Rustls(_) => (
            "tls_failed",
            "the TLS session with the storage host could not be established".to_string(),
        ),
        Error::Io(io) => match io_cause(io.kind()) {
            Some(cause) => cause,
            None => return GatewayError::from("the storage host could not be reached"),
        },
        _ => (
            "transport_failed",
            "the native transport failed before a usable answer arrived".to_string(),
        ),
    };
    GatewayError::unanswered(code, sentence)
}

/// The body ended with an I/O error mid-object. Only a lost or timed-out
/// connection is the network's; any other kind (this machine, a stop) is not.
pub fn body_failure(kind: std::io::ErrorKind) -> GatewayError {
    match io_cause(kind) {
        Some((code, sentence)) => {
            GatewayError::unanswered(code, format!("the download stopped: {sentence}"))
        }
        None => GatewayError::from(format!("the download stopped: {kind}")),
    }
}

fn io_cause(kind: std::io::ErrorKind) -> Option<(&'static str, String)> {
    use std::io::ErrorKind;
    let cause = match kind {
        ErrorKind::ConnectionRefused => (
            "connection_refused",
            "the connection to the storage host was refused on this machine's network path",
        ),
        ErrorKind::NetworkUnreachable
        | ErrorKind::HostUnreachable
        | ErrorKind::NetworkDown
        | ErrorKind::AddrNotAvailable => (
            "network_unreachable",
            "this machine has no network route to the storage host",
        ),
        ErrorKind::PermissionDenied => (
            "network_forbidden",
            "this execution context is not permitted to open network connections",
        ),
        ErrorKind::TimedOut => (
            "deadline_exceeded",
            "the storage host did not deliver the object within the download's progress budget",
        ),
        ErrorKind::ConnectionReset
        | ErrorKind::ConnectionAborted
        | ErrorKind::BrokenPipe
        | ErrorKind::UnexpectedEof
        | ErrorKind::NotConnected => (
            "connection_dropped",
            "the connection to the storage host was lost before the object completed",
        ),
        _ => return None,
    };
    Some((cause.0, cause.1.to_string()))
}

/// The bytes arrived whole but are not the object the head declares.
pub fn digest_mismatch() -> GatewayError {
    GatewayError::unanswered(
        DIGEST_MISMATCH,
        "the downloaded bytes do not match the digest the head declares",
    )
}

pub fn size_exceeded() -> GatewayError {
    GatewayError::unanswered(
        SIZE_MISMATCH,
        "the download exceeds the size the head declares",
    )
}

pub fn size_short() -> GatewayError {
    GatewayError::unanswered(
        SIZE_MISMATCH,
        "the download ended before the size the head declares",
    )
}
