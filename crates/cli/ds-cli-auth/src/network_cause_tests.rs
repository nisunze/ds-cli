//! Fault-injected native transport: each test makes a real loopback request
//! fail the way a sandbox, a refusing proxy, a stalled service or a degraded
//! gateway makes it fail, and reads the refusal the shared mapping returns.

use super::*;
use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::time::Duration;

fn transient() -> Failure {
    Failure::unavailable(
        "auth_transient",
        "the native authentication service is temporarily unavailable",
    )
    .remedy("retry without changing local state")
}

fn device_transient(retries: u8) -> Failure {
    let tail = if retries == 0 {
        String::new()
    } else {
        format!(" (retried {retries} times)")
    };
    Failure::unavailable(
        "device_auth_transient",
        format!("the fixed DS device authorization endpoint is temporarily unreachable{tail}"),
    )
    .remedy("retry without changing protected device state")
    .detail(json!({ "retries": retries }))
}

/// A direct agent: the test process's own proxy environment never decides
/// what a loopback fault looks like.
fn direct() -> ureq::Agent {
    ureq::Agent::config_builder()
        .proxy(None)
        .http_status_as_error(false)
        .timeout_global(Some(Duration::from_millis(400)))
        .build()
        .new_agent()
}

fn transport(failure: &Failure) -> &Value {
    &failure.detail_value().expect("detail")["transport"]
}

fn read_head(stream: &std::net::TcpStream) {
    let mut reader = BufReader::new(stream.try_clone().unwrap());
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" {
            break;
        }
    }
}

#[test]
fn a_refused_connect_names_this_machine_and_never_a_service_outage() {
    let address = {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.local_addr().unwrap()
    };
    request_started();
    let error = direct()
        .get(format!("http://{address}/api/v1/projects"))
        .call()
        .unwrap_err();
    assert_eq!(classify(error), TransportError::Unreachable);

    let failure = explain(transient(), Request::Native);
    assert_eq!(failure.code(), "auth_transient");
    assert!(
        failure.retryable(),
        "one call after a fresh lease may still heal"
    );
    // The sentence names the proxy when this process has one configured.
    assert!(
        failure
            .message()
            .starts_with("no DS service was reached: the ")
            && failure.message().contains("refused"),
        "{}",
        failure.message()
    );
    let named = transport(&failure);
    assert_eq!(named["cause"], "connection_refused");
    assert_eq!(named["reach"], "not_reached");
    assert_eq!(named["retry_after_s"], 15);
    assert_eq!(named["retry_here"], true);
    assert!(failure.remedy_text().unwrap().contains("normal shell"));
    assert!(
        failure
            .remedy_text()
            .unwrap()
            .contains("do not sign in again or reinstall")
    );
    assert_eq!(failure.next_commands(), [NETWORK_PROBE]);
    // Consumed: the next refusal is not explained by this request.
    assert_eq!(take(), None);
}

#[test]
fn a_whole_refresh_curve_refused_here_is_not_retried_from_the_same_context() {
    let address = {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.local_addr().unwrap()
    };
    for _ in 0..4 {
        let error = direct()
            .post(format!("http://{address}/api/v1/auth/device/refresh"))
            .send(b"{}".as_slice())
            .unwrap_err();
        classify(error);
    }
    let failure = explain(device_transient(3), Request::DeviceRefresh { retries: 3 });
    assert_eq!(failure.code(), "device_auth_transient");
    assert!(!failure.retryable());
    assert!(
        failure.message().ends_with("(retried 3 times)"),
        "{}",
        failure.message()
    );
    let named = transport(&failure);
    assert_eq!(named["request"], "device_refresh");
    assert_eq!(named["retry_here"], false);
    assert!(named.get("retry_after_s").is_none());
    // The detail the device mapping already carried survives the merge.
    assert_eq!(failure.detail_value().unwrap()["retries"], 3);
    let remedy = failure.remedy_text().unwrap();
    assert!(remedy.contains("execution context cannot reach the DS gateway"));
    assert!(remedy.contains("do not sign in again, relink or reinstall"));
    assert_eq!(failure.next_commands(), [NETWORK_PROBE]);
}

#[test]
fn a_proxy_that_refuses_the_tunnel_is_named_as_the_proxy() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let proxy = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        read_head(&stream);
        stream
            .write_all(b"HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
            .unwrap();
    });
    let agent = ureq::Agent::config_builder()
        .proxy(Some(
            ureq::Proxy::new(&format!("http://{address}")).unwrap(),
        ))
        .timeout_global(Some(Duration::from_secs(5)))
        .build()
        .new_agent();
    let error = agent
        .get("https://gateway.invalid/api/v1/projects")
        .call()
        .unwrap_err();
    assert_eq!(classify(error), TransportError::Unreachable);
    proxy.join().unwrap();
    let failure = explain(transient(), Request::Native);
    assert_eq!(transport(&failure)["cause"], "proxy_refused");
    assert!(
        failure
            .message()
            .contains("HTTP proxy configured for this process")
    );
}

#[test]
fn a_service_that_takes_the_request_and_stalls_is_a_deadline_not_an_outage_here() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let held = std::thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        read_head(&stream);
        std::thread::sleep(Duration::from_millis(900));
        drop(stream);
    });
    let error = direct()
        .get(format!("http://{address}/api/v1/forms"))
        .call()
        .unwrap_err();
    assert_eq!(classify(error), TransportError::TimedOut);
    held.join().unwrap();
    let failure = explain(transient(), Request::Native);
    let named = transport(&failure);
    assert_eq!(named["cause"], "deadline_exceeded");
    assert_eq!(named["reach"], "no_answer");
    assert_eq!(named["retry_after_s"], 15);
    assert!(failure.retryable());
    assert_eq!(failure.next_commands(), ["ds sre overview"]);
}

#[test]
fn a_degraded_gateway_answer_carries_its_status_and_bounded_retry_after() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        read_head(&stream);
        stream
            .write_all(b"HTTP/1.1 503 Service Unavailable\r\nRetry-After: 7\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}")
            .unwrap();
    });
    let response = direct()
        .get(format!("http://{address}/api/v1/documents"))
        .call()
        .unwrap();
    let answered = crate::transport::bounded(response, 1024).unwrap();
    assert_eq!(answered.status, 503);
    server.join().unwrap();
    let failure = explain(transient(), Request::Native);
    let named = transport(&failure);
    assert_eq!(named["cause"], "service_status");
    assert_eq!(named["reach"], "answered");
    assert_eq!(named["http_status"], 503);
    assert_eq!(named["retry_after_s"], 7);
    assert!(failure.message().contains("answered HTTP 503"));
    assert!(failure.message().contains("not rejecting this credential"));
    assert!(
        failure
            .remedy_text()
            .unwrap()
            .starts_with("retry after 7 seconds")
    );

    answered_with(503, Some("86400"));
    let clamped = explain(transient(), Request::Native);
    assert_eq!(
        transport(&clamped)["retry_after_s"],
        RETRY_AFTER_MAX_SECONDS
    );
}

fn answered_with(status: u16, retry_after: Option<&str>) {
    answered(status, retry_after);
}

#[test]
fn a_usable_answer_clears_an_earlier_attempt_s_cause() {
    // The curve's first attempt was refused; its retry reached the service.
    access_lease_spent();
    answered(200, None);
    let failure = explain(transient(), Request::Native);
    assert_eq!(failure.message(), transient().message());
    assert!(failure.detail_value().is_none());
}

#[test]
fn a_spent_access_lease_says_nothing_was_sent() {
    access_lease_spent();
    let failure = explain(transient(), Request::Native);
    let named = transport(&failure);
    assert_eq!(named["cause"], "access_lease_spent");
    assert_eq!(named["reach"], "not_sent");
    assert_eq!(named["retry_after_s"], 0);
    assert!(failure.message().starts_with("no request was sent"));
    assert!(
        failure
            .remedy_text()
            .unwrap()
            .starts_with("rerun the command")
    );
}

#[test]
fn without_a_recorded_cause_or_for_another_code_the_refusal_is_unchanged() {
    request_started();
    let unexplained = explain(transient(), Request::Native);
    assert_eq!(unexplained.message(), transient().message());
    assert_eq!(unexplained.remedy_text(), transient().remedy_text());
    assert!(unexplained.detail_value().is_none());
    assert!(unexplained.retryable());

    access_lease_spent();
    let rejected = Failure::unauthorized("auth_rejected", "rejected");
    let kept = explain(rejected, Request::Native);
    assert_eq!(kept.code(), "auth_rejected");
    assert_eq!(kept.message(), "rejected");
    assert!(kept.detail_value().is_none());
    assert_eq!(take(), None, "a cause never outlives the refusal it met");
}

#[test]
fn a_route_s_own_sentence_and_remedy_are_kept_and_the_cause_follows() {
    record(Cause::ConnectionDropped, None);
    let route = Failure::unavailable("auth_transient", "Solar publication stalled")
        .remedy("retry the exact sealed publication without changing local state");
    let failure = explain(route, Request::Native);
    assert_eq!(
        failure.message(),
        "Solar publication stalled: the connection to the DS gateway was lost before its answer completed"
    );
    assert_eq!(
        failure.remedy_text(),
        Some("retry the exact sealed publication without changing local state")
    );
    assert_eq!(transport(&failure)["reach"], "dropped");
}

#[test]
fn a_cause_recorded_on_another_thread_never_explains_this_one() {
    request_started();
    std::thread::spawn(access_lease_spent).join().unwrap();
    assert_eq!(take(), None);
}

#[test]
fn operating_system_refusals_map_to_closed_causes() {
    use std::io::ErrorKind;
    for (kind, cause) in [
        (ErrorKind::PermissionDenied, Cause::NetworkForbidden),
        (ErrorKind::NetworkUnreachable, Cause::NetworkUnreachable),
        (ErrorKind::HostUnreachable, Cause::NetworkUnreachable),
        (ErrorKind::ConnectionReset, Cause::ConnectionDropped),
        (ErrorKind::UnexpectedEof, Cause::ConnectionDropped),
        (ErrorKind::TimedOut, Cause::DeadlineExceeded),
        (ErrorKind::Other, Cause::Other),
    ] {
        assert_eq!(
            cause_of(&ureq::Error::Io(std::io::Error::from(kind))),
            cause,
            "{kind:?}"
        );
    }
    assert_eq!(cause_of(&ureq::Error::HostNotFound), Cause::NameUnresolved);
    assert_eq!(
        cause_of(&ureq::Error::Timeout(ureq::Timeout::Connect)),
        Cause::ConnectTimedOut
    );
    assert_eq!(
        cause_of(&ureq::Error::Timeout(ureq::Timeout::RecvResponse)),
        Cause::DeadlineExceeded
    );
    assert_eq!(
        cause_of(&ureq::Error::ConnectProxyFailed("403".into())),
        Cause::ProxyRefused
    );
    // A forbidden socket is the execution context, never a service.
    assert_eq!(Cause::NetworkForbidden.reach(), Reach::NotReached);
}
