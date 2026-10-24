//! Why the last native request on this thread got no usable answer.
//!
//! ds-client-core sees only `TransportError::{Unreachable, TimedOut}`, which
//! is all its retry curve needs. A caller reading `auth_transient` or
//! `device_auth_transient` needs more: did any DS service answer at all, and
//! can repeating the command from this execution context work? An agent
//! sandbox, a refusing proxy or an offline link refused every attempt on this
//! machine, and the old sentence ("the authentication service is temporarily
//! unavailable", "retry") sent callers into a retry loop against a service
//! that was never contacted, and suggested sign-in, relinking or reinstalls
//! when the credential was intact.
//!
//! The native adapters are the only code that sees the real socket, proxy and
//! DNS outcome. Each request records one closed cause here, either from its
//! error (`classify`) or from its answer (`answered`), on the thread that made
//! it. The shared failure mapping then names that cause in the refusal it
//! was already returning. The code, class and every declared field stay the
//! same, and the explanation is an additive `detail.transport` object. Nothing secret
//! crosses: a closed token, an HTTP status, a bounded retry hint, and whether
//! a process proxy is configured (never its address).

use std::cell::Cell;

use ds_cli_contract::Failure;
use ds_client_core::TransportError;
use serde_json::{Value, json};

/// The pause the kernel's retry curve ends on (`ds_command_kernel::retry`),
/// in seconds: the hint every transient that can heal by itself carries.
const RETRY_AFTER_SECONDS: u64 = ds_command_kernel::retry::PAUSES_MS[2] / 1000;
/// The longest service `Retry-After` this hint repeats; a longer one is a
/// statement about an outage, not a retry pause, and is clamped.
const RETRY_AFTER_MAX_SECONDS: u64 = 300;
/// The remedy the shared mappings attach before a cause is known. A route
/// that authored its own remedy keeps it.
const GENERIC_REMEDIES: [&str; 2] = [
    "retry without changing local state",
    "retry without changing protected device state",
];
const GENERIC_MESSAGES: [&str; 2] = [
    "the native authentication service is temporarily unavailable",
    "the fixed DS device authorization endpoint is temporarily unreachable",
];
const NETWORK_PROBE: &str = "ds auth project list --limit 1 --lane <stable|canary>";

/// One closed reason a native request ended without a usable answer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Cause {
    /// The gateway host name did not resolve here.
    NameUnresolved,
    /// The process's configured HTTP proxy refused or failed the tunnel.
    ProxyRefused,
    /// The connection was refused on this machine's network path.
    ConnectionRefused,
    /// No route: the network is down or unreachable from here.
    NetworkUnreachable,
    /// The operating system refused to open the socket: a sandbox or policy
    /// forbids network access to this execution context.
    NetworkForbidden,
    /// The connection did not open within the connect deadline.
    ConnectTimedOut,
    /// The connection opened and was then lost before the answer completed.
    ConnectionDropped,
    /// TLS could not be established (often a proxy or TLS inspection).
    Tls,
    /// The service accepted the request and did not answer within the
    /// call's deadline.
    DeadlineExceeded,
    /// The gateway answered a not-now status (408, 425, 429 or 5xx).
    ServiceStatus(u16),
    /// No request was sent: the command held its short-lived device access
    /// lease past its expiry.
    AccessLeaseSpent,
    /// A transport failure outside the classes above.
    Other,
}

/// How far a request got, the part of a cause a caller decides on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Reach {
    /// No request left this process.
    NotSent,
    /// No connection to the DS gateway was established from this machine.
    NotReached,
    /// A connection was established and then lost.
    Dropped,
    /// The service took the request and did not answer in time.
    NoAnswer,
    /// The gateway answered with a not-now status.
    Answered,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Observed {
    cause: Cause,
    retry_after: Option<u64>,
    proxy_configured: bool,
}

/// Which request a refusal came from, when the mapping knows it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Request {
    /// A device access-lease refresh, retried by the kernel's curve.
    DeviceRefresh { retries: u8 },
    /// Any other native call; the mapping does not know which request
    /// (a Firebase refresh or a service read) failed.
    Native,
}

thread_local! {
    static LAST: Cell<Option<Observed>> = const { Cell::new(None) };
}

impl Cause {
    pub(crate) const fn token(self) -> &'static str {
        match self {
            Self::NameUnresolved => "name_unresolved",
            Self::ProxyRefused => "proxy_refused",
            Self::ConnectionRefused => "connection_refused",
            Self::NetworkUnreachable => "network_unreachable",
            Self::NetworkForbidden => "network_forbidden",
            Self::ConnectTimedOut => "connect_timed_out",
            Self::ConnectionDropped => "connection_dropped",
            Self::Tls => "tls_failed",
            Self::DeadlineExceeded => "deadline_exceeded",
            Self::ServiceStatus(_) => "service_status",
            Self::AccessLeaseSpent => "access_lease_spent",
            Self::Other => "transport_failed",
        }
    }

    pub(crate) const fn reach(self) -> Reach {
        match self {
            Self::AccessLeaseSpent => Reach::NotSent,
            Self::NameUnresolved
            | Self::ProxyRefused
            | Self::ConnectionRefused
            | Self::NetworkUnreachable
            | Self::NetworkForbidden
            | Self::ConnectTimedOut => Reach::NotReached,
            Self::ConnectionDropped | Self::Tls | Self::Other => Reach::Dropped,
            Self::DeadlineExceeded => Reach::NoAnswer,
            Self::ServiceStatus(_) => Reach::Answered,
        }
    }

    fn sentence(self, proxy_configured: bool) -> String {
        match self {
            Self::NameUnresolved => {
                "this machine could not resolve the DS gateway host name".into()
            }
            Self::ProxyRefused => {
                "the HTTP proxy configured for this process refused the tunnel to the DS gateway"
                    .into()
            }
            Self::ConnectionRefused if proxy_configured => {
                "the HTTP proxy configured for this process refused the connection".into()
            }
            Self::ConnectionRefused => {
                "the connection to the DS gateway was refused on this machine's network path"
                    .into()
            }
            Self::NetworkUnreachable => {
                "this machine has no network route to the DS gateway".into()
            }
            Self::NetworkForbidden => {
                "this execution context is not permitted to open network connections".into()
            }
            Self::ConnectTimedOut => {
                "the connection to the DS gateway did not open within the connect deadline".into()
            }
            Self::ConnectionDropped => {
                "the connection to the DS gateway was lost before its answer completed".into()
            }
            Self::Tls => {
                "the TLS session with the DS gateway could not be established".into()
            }
            Self::DeadlineExceeded => {
                "the DS service took the request and did not answer within the call's deadline"
                    .into()
            }
            Self::ServiceStatus(status) => format!(
                "the DS gateway answered HTTP {status}: the service is failing or rate-limited, not rejecting this credential"
            ),
            Self::AccessLeaseSpent => {
                "no request was sent: this command held its short-lived device access lease past its expiry"
                    .into()
            }
            Self::Other => "the native transport failed before a usable answer arrived".into(),
        }
    }
}

impl Reach {
    const fn token(self) -> &'static str {
        match self {
            Self::NotSent => "not_sent",
            Self::NotReached => "not_reached",
            Self::Dropped => "dropped",
            Self::NoAnswer => "no_answer",
            Self::Answered => "answered",
        }
    }
}

fn proxy_configured() -> bool {
    ureq::Proxy::try_from_env().is_some()
}

fn record(cause: Cause, retry_after: Option<u64>) {
    let observed = Observed {
        cause,
        retry_after,
        proxy_configured: proxy_configured(),
    };
    LAST.with(|last| last.set(Some(observed)));
}

/// A new request starts: nothing an earlier request saw describes it.
pub(crate) fn request_started() {
    LAST.with(|last| last.set(None));
}

/// Classify one ureq failure for the kernel and record its cause here.
pub(crate) fn classify(error: ureq::Error) -> TransportError {
    let cause = cause_of(&error);
    record(cause, None);
    match cause {
        Cause::ConnectTimedOut | Cause::DeadlineExceeded => TransportError::TimedOut,
        _ => TransportError::Unreachable,
    }
}

/// The gateway answered: a not-now status is recorded with its bounded
/// `Retry-After`; any other answer clears the cause, because whatever the
/// caller decides next is the service's answer, not the transport's.
pub(crate) fn answered(status: u16, retry_after: Option<&str>) {
    if ds_client_core::weak_network::transient_status(status) {
        let hint = retry_after
            .and_then(|value| value.trim().parse::<u64>().ok())
            .map(|seconds| seconds.clamp(1, RETRY_AFTER_MAX_SECONDS));
        record(Cause::ServiceStatus(status), hint);
    } else {
        request_started();
    }
}

/// An answer's body could not be read to its end.
pub(crate) fn body_failed(error: &std::io::Error) {
    let cause = match error.kind() {
        std::io::ErrorKind::TimedOut => Cause::DeadlineExceeded,
        std::io::ErrorKind::ConnectionReset
        | std::io::ErrorKind::ConnectionAborted
        | std::io::ErrorKind::BrokenPipe
        | std::io::ErrorKind::UnexpectedEof
        | std::io::ErrorKind::NotConnected => Cause::ConnectionDropped,
        // An over-limit body is the contract's refusal, not the network's.
        _ => return request_started(),
    };
    record(cause, None);
}

/// A device call was refused locally because its access lease was spent.
pub(crate) fn access_lease_spent() {
    record(Cause::AccessLeaseSpent, None);
}

/// Take what the last request on this thread recorded, once.
pub(crate) fn take() -> Option<Observed> {
    LAST.with(Cell::take)
}

/// What the last request on this thread recorded, as a Sync Center gateway
/// failure names it: the closed cause token and its bounded sentence, taken
/// once. The sync loop keeps both on the receipt of the step or pass that
/// failed (`failure_code` and `detail`), so a held row says why it waits.
pub(crate) fn take_gateway_cause() -> Option<(&'static str, String)> {
    let observed = take()?;
    let cause = observed.cause;
    let sentence = cause.sentence(observed.proxy_configured);
    let lead = match cause.reach() {
        Reach::NotReached => format!("no DS service was reached: {sentence}"),
        _ => sentence,
    };
    Some((cause.token(), lead))
}

fn cause_of(error: &ureq::Error) -> Cause {
    use ureq::Error;
    use ureq::Timeout;
    match error {
        Error::Timeout(Timeout::Resolve) => Cause::NameUnresolved,
        Error::Timeout(Timeout::Connect) => Cause::ConnectTimedOut,
        Error::Timeout(_) => Cause::DeadlineExceeded,
        Error::HostNotFound => Cause::NameUnresolved,
        Error::ConnectProxyFailed(_) | Error::InvalidProxyUrl => Cause::ProxyRefused,
        Error::ConnectionFailed => Cause::ConnectionRefused,
        Error::Tls(_) | Error::TlsRequired | Error::Rustls(_) => Cause::Tls,
        Error::Io(io) => io_cause(io.kind()),
        _ => Cause::Other,
    }
}

fn io_cause(kind: std::io::ErrorKind) -> Cause {
    use std::io::ErrorKind;
    match kind {
        ErrorKind::ConnectionRefused => Cause::ConnectionRefused,
        ErrorKind::NetworkUnreachable
        | ErrorKind::HostUnreachable
        | ErrorKind::NetworkDown
        | ErrorKind::AddrNotAvailable => Cause::NetworkUnreachable,
        ErrorKind::PermissionDenied => Cause::NetworkForbidden,
        ErrorKind::TimedOut => Cause::DeadlineExceeded,
        ErrorKind::ConnectionReset
        | ErrorKind::ConnectionAborted
        | ErrorKind::BrokenPipe
        | ErrorKind::UnexpectedEof
        | ErrorKind::NotConnected => Cause::ConnectionDropped,
        _ => Cause::Other,
    }
}

/// Name the recorded cause in a transient refusal the shared mapping built.
///
/// Only `auth_transient` and `device_auth_transient` are explained, and only
/// when this thread recorded a cause; anything else returns unchanged. The
/// code, class and existing detail fields are preserved. A generic sentence
/// or remedy is replaced, and a route's own sentence or remedy is kept.
pub(crate) fn explain(failure: Failure, request: Request) -> Failure {
    // Consumed either way: a cause describes at most one refusal.
    let observed = take();
    match observed {
        Some(observed) if matches!(failure.code(), "auth_transient" | "device_auth_transient") => {
            explain_observed(failure, request, observed)
        }
        _ => failure,
    }
}

pub(crate) fn explain_observed(failure: Failure, request: Request, observed: Observed) -> Failure {
    let cause = observed.cause;
    let reach = cause.reach();
    let sentence = cause.sentence(observed.proxy_configured);
    // A whole curve of attempts refused on this machine is not a blink: the
    // same command from the same context fails the same way.
    let refused_here = reach == Reach::NotReached
        && matches!(request, Request::DeviceRefresh { retries } if retries >= 1);
    let retry_after = match reach {
        Reach::NotSent => Some(0),
        _ if refused_here => None,
        Reach::Answered => Some(observed.retry_after.unwrap_or(RETRY_AFTER_SECONDS)),
        _ => Some(RETRY_AFTER_SECONDS),
    };

    let mut transport = json!({
        "cause": cause.token(),
        "reach": reach.token(),
        "proxy_configured": observed.proxy_configured,
        "retry_here": !refused_here,
    });
    if let Cause::ServiceStatus(status) = cause {
        transport["http_status"] = json!(status);
    }
    if let Request::DeviceRefresh { .. } = request {
        transport["request"] = json!("device_refresh");
    }
    if let Some(seconds) = retry_after {
        transport["retry_after_s"] = json!(seconds);
    }

    let lead = match reach {
        Reach::NotReached => format!("no DS service was reached: {sentence}"),
        _ => sentence,
    };
    // A generic sentence is replaced, keeping its "(retried N times)" tail;
    // a route's own sentence is kept and the cause follows it.
    let message = match GENERIC_MESSAGES
        .iter()
        .find_map(|generic| failure.message().strip_prefix(generic))
    {
        Some(tail) => format!("{lead}{tail}"),
        None => format!("{}: {lead}", failure.message()),
    };
    let keeps_remedy = failure
        .remedy_text()
        .is_some_and(|remedy| !GENERIC_REMEDIES.contains(&remedy));
    let mut detail = match failure.detail_value() {
        Some(Value::Object(existing)) => Value::Object(existing.clone()),
        Some(other) => json!({ "owner_detail": other }),
        None => json!({}),
    };
    detail["transport"] = transport;
    let mut failure = failure.with_message(message).detail(detail);
    if refused_here {
        failure = failure.not_retryable();
    }
    if keeps_remedy {
        return failure;
    }
    let after = retry_after.unwrap_or(RETRY_AFTER_SECONDS);
    match reach {
        Reach::NotSent => failure.remedy(
            "rerun the command: each run renews the device access lease; nothing local needs repair",
        ),
        Reach::NotReached if refused_here => failure
            .remedy(
                "this execution context cannot reach the DS gateway (sandbox, proxy or offline network); rerun the same command from a normal shell for this OS user, or allow this context to reach the gateway; retrying here fails the same way, and the protected credential is intact: do not sign in again, relink or reinstall",
            )
            .next(NETWORK_PROBE),
        Reach::NotReached => failure
            .remedy(format!(
                "retry once after {after} seconds; if it repeats, rerun from a normal shell for this OS user or allow this context to reach the DS gateway; do not sign in again or reinstall"
            ))
            .next(NETWORK_PROBE),
        Reach::Dropped => failure.remedy(format!(
            "retry after {after} seconds without changing local state; if it repeats from a normal shell too, a proxy or TLS-inspecting network on this path is interfering"
        )),
        Reach::NoAnswer | Reach::Answered => failure
            .remedy(format!(
                "retry after {after} seconds without changing local state; the credential is fine, and a repeat means the service is degraded"
            ))
            .next("ds sre overview"),
    }
}

#[cfg(test)]
#[path = "network_cause_tests.rs"]
mod network_cause_tests;
