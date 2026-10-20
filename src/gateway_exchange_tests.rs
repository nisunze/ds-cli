use super::maps::{Config3d, GoogleDarkSession, Maps3dConfig};
use super::*;
use serde_json::json;

fn dark(project: &str) -> GoogleDarkSession {
    GoogleDarkSession {
        project: project.into(),
    }
}

fn signed_in() -> Facts {
    Facts {
        now_ms: 1_760_000_000_000,
        has_session: true,
        offline_mode: false,
    }
}

fn fetched<C: Exchange>(step: Step<C>) -> (Request, Continuation<C>) {
    match step {
        Step::Fetch {
            request,
            continuation,
        } => (request, continuation),
        _ => panic!("expected a fetch"),
    }
}

fn refusal<C: Exchange>(step: Step<C>) -> Refusal {
    match step {
        Step::Refused(refusal) => refusal,
        Step::Done(value) => panic!("expected a refusal, got {value:?}"),
        _ => panic!("expected a refusal"),
    }
}

fn done<C: Exchange>(step: Step<C>) -> C::Output {
    match step {
        Step::Done(value) => value,
        Step::Refused(refusal) => panic!("expected an answer, got {refusal:?}"),
        _ => panic!("expected an answer"),
    }
}

fn answer<C: Exchange>(call: C, status: u16, body: Value) -> Step<C> {
    let (_, continuation) = fetched(start(call));
    resume(
        continuation,
        Observed::Response {
            status,
            content_type: Some("application/json; charset=utf-8".into()),
            retry_after: None,
        },
        body.to_string().as_bytes(),
        &signed_in(),
    )
}

fn session_body() -> Value {
    json!({"success": true, "message": "Google Dark map session", "data": {
        "basemap": {"id": "google-dark", "label": "Dark (Google)", "type": "raster",
            "tiles": ["https://tile.googleapis.com/v1/2dtiles/{z}/{x}/{y}?session=s&key=k"],
            "tileSize": 256, "attribution": "Map data © Google"},
        "expires_at": "1760003600"}})
}

fn config_json() -> Value {
    json!({"default_provider": "auto",
        "google": {"available": false, "root_url": "", "attribution": "Powered by Google"},
        "aws_terrain": {"available": true, "tiles_url": "https://s3.amazonaws.com/elevation-tiles-prod/terrarium/{z}/{x}/{y}.png",
            "encoding": "terrarium", "max_zoom": 15, "default_exaggeration": 1.5, "attribution": "Terrain — AWS Open Data"}})
}

#[test]
fn the_dark_session_names_its_request_and_bounds() {
    let (request, continuation) = fetched(start(dark("it_rwanda")));
    assert_eq!(
        serde_json::to_value(&request).unwrap(),
        json!({"method": "POST", "path": "/api/v1/maps2d",
            "body": {"action": "get_google_dark_session", "ds_project": "it_rwanda"},
            "timeout_ms": 30000, "response_cap_bytes": 262144})
    );
    assert_eq!(continuation.attempt, 0);
}

#[test]
fn an_invalid_project_is_refused_before_any_request() {
    for project in ["", "a/b", "two words", "..", "NUL"] {
        assert_eq!(refusal(start(dark(project))).code, PROJECT_INVALID);
    }
}

#[test]
fn a_dark_session_is_admitted_as_its_typed_projection() {
    let session = done(answer(dark("it_rwanda"), 200, session_body())).expect("tiles on offer");
    assert_eq!(session.basemap.id, "google-dark");
    assert_eq!(session.basemap.tile_size, Some(256));
    assert_eq!(session.expires_at, "1760003600");
    assert_eq!(
        serde_json::to_value(&session).unwrap()["basemap"]["tileSize"],
        256
    );
}

#[test]
fn a_project_without_dark_tiles_reads_as_none() {
    for body in [
        json!({"success": true, "data": {"basemap": {"tiles": []}}}),
        json!({"success": true, "data": {}}),
        json!({}),
    ] {
        assert_eq!(done(answer(dark("p"), 200, body)), None);
    }
}

#[test]
fn a_malformed_dark_session_is_refused_not_rendered() {
    let mut body = session_body();
    body["data"]["basemap"]
        .as_object_mut()
        .unwrap()
        .remove("label");
    assert_eq!(refusal(answer(dark("p"), 200, body)).code, maps::INVALID);
    let mut blank = session_body();
    blank["data"]["basemap"]["tiles"] = json!([" "]);
    assert_eq!(refusal(answer(dark("p"), 200, blank)).code, maps::INVALID);
}

#[test]
fn the_3d_configuration_is_admitted_whole_or_refused() {
    let (request, _) = fetched(start(Maps3dConfig));
    assert_eq!(request.path, "/api/v1/maps3d");
    assert_eq!(request.body, Some(json!({"action": "get_config"})));
    let config: Config3d = done(answer(
        Maps3dConfig,
        200,
        json!({"success": true, "data": config_json()}),
    ));
    assert_eq!(serde_json::to_value(&config).unwrap(), config_json());
    let mut missing = config_json();
    missing.as_object_mut().unwrap().remove("aws_terrain");
    assert_eq!(
        refusal(answer(Maps3dConfig, 200, json!({"data": missing}))).code,
        maps::INVALID
    );
}

#[test]
fn a_backend_refusal_carries_its_code_words_details_and_report() {
    let refused = refusal(answer(
        dark("it_rwanda"),
        400,
        json!({"success": false, "error": {"code": "VALIDATION_FAILED",
            "message": "ds_project is required", "details": {"field": "ds_project"}}}),
    ));
    assert_eq!(refused.code, "VALIDATION_FAILED");
    assert_eq!(refused.message, "ds_project is required");
    assert_eq!(refused.status, Some(400));
    assert_eq!(refused.details, Some(json!({"field": "ds_project"})));
    assert!(!refused.transient && !refused.retryable && !refused.signed_out);
    let report = refused.report.expect("a 400 is worth a report");
    assert_eq!(report.action, "get_google_dark_session");
    assert_eq!(report.path, "/api/v1/maps2d");
    assert_eq!(report.ds_project.as_deref(), Some("it_rwanda"));
    assert_eq!(report.deadline_s, 30);
}

#[test]
fn a_busy_service_is_transient_and_not_reported() {
    let refused = refusal(answer(
        Maps3dConfig,
        503,
        json!({"error": {"code": "SERVICE_UNAVAILABLE", "message": "Google Map Tiles API is not configured"}}),
    ));
    assert_eq!(refused.code, "SERVICE_UNAVAILABLE");
    assert!(refused.transient && refused.retryable);
    assert!(refused.report.is_none());
}

#[test]
fn an_unnamed_refusal_is_coded_by_its_status() {
    let refused = refusal(answer(Maps3dConfig, 502, json!({})));
    assert_eq!(refused.code, "HTTP_502");
    assert_eq!(refused.message, "Request failed with status 502");
}

#[test]
fn a_rewritten_message_keeps_the_backend_text_beside_it() {
    let refused = refusal(answer(
        Maps3dConfig,
        500,
        json!({"error": "rpc error: invalid_grant: reauth required"}),
    ));
    assert!(refused
        .message
        .starts_with("Local service authorization expired"));
    assert_eq!(
        refused.technical.as_deref(),
        Some("rpc error: invalid_grant: reauth required")
    );
}

#[test]
fn a_refusal_after_the_session_ended_is_inert() {
    let (_, continuation) = fetched(start(Maps3dConfig));
    let refused = refusal(resume(
        continuation,
        Observed::Response {
            status: 401,
            content_type: Some("application/json".into()),
            retry_after: None,
        },
        br#"{"error":{"code":"INVALID_TOKEN","message":"token expired"}}"#,
        &Facts {
            has_session: false,
            ..signed_in()
        },
    ));
    assert_eq!(refused.code, SIGNED_OUT);
    assert!(refused.signed_out);
    assert!(refused.report.is_none() && refused.technical.is_none());
    let live = refusal(answer(
        Maps3dConfig,
        401,
        json!({"error": {"code": "INVALID_TOKEN", "message": "token expired"}}),
    ));
    assert_eq!(live.code, "INVALID_TOKEN");
    assert!(!live.signed_out);
}

fn plan_with_retries<C: Exchange>(call: &C, retries: u32) -> Plan {
    Plan {
        retries,
        ..call.plan().unwrap()
    }
}

#[test]
fn a_resendable_status_waits_then_sends_again_within_the_budget() {
    let plan = plan_with_retries(&Maps3dConfig, 2);
    let body = json!({"error": "busy"});
    let Step::Wait {
        delay_ms,
        jitter_ms,
        continuation,
    } = answered(Maps3dConfig, &plan, 0, 503, None, &body, &signed_in())
    else {
        panic!("a 503 within the budget waits")
    };
    assert_eq!((delay_ms, jitter_ms), (1_800, 400));
    assert_eq!(continuation.attempt, 1);
    let (request, again) = fetched(resume(continuation, Observed::Waited, &[], &signed_in()));
    assert_eq!(request, plan.request);
    assert_eq!(again.attempt, 1);
    assert!(matches!(
        answered(Maps3dConfig, &plan, 2, 503, None, &body, &signed_in()),
        Step::Refused(_)
    ));
    // A 500 may have applied its effect: never sent again.
    assert!(matches!(
        answered(Maps3dConfig, &plan, 0, 500, None, &body, &signed_in()),
        Step::Refused(_)
    ));
}

#[test]
fn a_named_retry_after_is_waited_exactly() {
    let plan = plan_with_retries(&Maps3dConfig, 1);
    let Step::Wait {
        delay_ms,
        jitter_ms,
        ..
    } = answered(
        Maps3dConfig,
        &plan,
        0,
        429,
        Some("7".into()),
        &json!({}),
        &signed_in(),
    )
    else {
        panic!("a 429 within the budget waits")
    };
    assert_eq!((delay_ms, jitter_ms), (7_000, 0));
}

fn failure(cause: Failure, message: &str) -> Step<Maps3dConfig> {
    let (_, continuation) = fetched(start(Maps3dConfig));
    resume(
        continuation,
        Observed::Failure {
            cause,
            message: Some(message.into()),
        },
        &[],
        &signed_in(),
    )
}

#[test]
fn a_failed_link_is_unreachable_and_its_outcome_unknown() {
    let refused = refusal(failure(Failure::Network, "Failed to fetch"));
    assert_eq!(refused.code, NETWORK_ERROR);
    assert!(refused
        .message
        .starts_with("Data Solutions API is not reachable"));
    assert_eq!(refused.technical.as_deref(), Some("Failed to fetch"));
    assert!(refused.outcome_unknown && refused.transient);
    assert!(refused.report.is_none());
    let offline = refusal(failure(Failure::Offline, "Failed to fetch"));
    assert_eq!(offline.code, NETWORK_OFFLINE);
    assert!(offline.message.contains("this browser is offline"));
}

#[test]
fn a_failed_link_is_sent_again_within_the_budget() {
    let plan = plan_with_retries(&Maps3dConfig, 1);
    let step = failed(Maps3dConfig, &plan, 0, Failure::Network, None, &signed_in());
    assert!(matches!(step, Step::Wait { .. }));
    let spent = failed(Maps3dConfig, &plan, 1, Failure::Network, None, &signed_in());
    assert_eq!(refusal(spent).code, NETWORK_ERROR);
}

#[test]
fn a_timeout_is_surfaced_never_resent() {
    let plan = plan_with_retries(&Maps3dConfig, 3);
    let timed_out = refusal(failed(
        Maps3dConfig,
        &plan,
        0,
        Failure::Timeout,
        None,
        &signed_in(),
    ));
    assert_eq!(timed_out.code, REQUEST_TIMEOUT);
    assert_eq!(timed_out.message, "Request timed out after 30s");
    assert!(timed_out.outcome_unknown);
}

#[test]
fn an_answer_past_its_bound_is_refused() {
    let (request, continuation) = fetched(start(Maps3dConfig));
    let oversized = vec![b' '; request.response_cap_bytes as usize + 1];
    let refused = refusal(resume(
        continuation,
        Observed::Response {
            status: 200,
            content_type: Some("application/json".into()),
            retry_after: None,
        },
        &oversized,
        &signed_in(),
    ));
    assert_eq!(refused.code, RESPONSE_TOO_LARGE);
    assert_eq!(
        refusal(failure(Failure::Oversized, "stream exceeded")).code,
        RESPONSE_TOO_LARGE
    );
}

#[test]
fn offline_mode_refuses_with_the_kernels_own_sentence() {
    let refused = offline_refusal();
    assert_eq!(refused.code, NETWORK_OFFLINE);
    assert_eq!(refused.message, crate::connectivity::OFFLINE_MESSAGE);
}

#[test]
fn bodies_are_read_the_way_the_gateway_serves_them() {
    assert_eq!(decode_body(204, Some("application/json"), b"x"), json!({}));
    assert_eq!(decode_body(200, Some("application/json"), b""), json!({}));
    assert_eq!(
        decode_body(200, Some("application/json"), b"[1]"),
        json!([1])
    );
    assert_eq!(
        decode_body(200, Some("application/json"), b"7"),
        json!({"data": 7})
    );
    assert_eq!(
        decode_body(200, Some("application/json"), b"{bad"),
        json!({})
    );
    assert_eq!(
        decode_body(502, Some("text/html"), b"Bad gateway"),
        json!({"message": "Bad gateway"})
    );
    assert_eq!(decode_body(502, None, b""), json!({}));
}

#[test]
fn a_link_cut_by_offline_mode_is_the_offline_refusal_never_a_retry() {
    let plan = plan_with_retries(&Maps3dConfig, 3);
    let offline = Facts {
        offline_mode: true,
        ..signed_in()
    };
    let refused = refusal(failed(
        Maps3dConfig,
        &plan,
        0,
        Failure::Network,
        Some("Failed to fetch".into()),
        &offline,
    ));
    assert_eq!(refused, offline_refusal());
}

/// A bounded NDJSON read, as a tabular stream declares it.
struct Rows;

impl Exchange for Rows {
    type Output = Value;

    fn plan(&self) -> Result<Plan, Refusal> {
        Ok(
            Request::post("/api/v1/data", json!({"action": "query_table"}))
                .ndjson(3)
                .into(),
        )
    }

    fn admit(self, _status: u16, body: Value) -> Result<Admitted<Self>, Refusal> {
        Ok(Admitted::Done(body))
    }
}

fn streamed(status: u16, body: &[u8]) -> Step<Rows> {
    streamed_as(status, "application/x-ndjson", body)
}

fn streamed_as(status: u16, content_type: &str, body: &[u8]) -> Step<Rows> {
    let (_, continuation) = fetched(start(Rows));
    resume(
        continuation,
        Observed::Response {
            status,
            content_type: Some(content_type.into()),
            retry_after: None,
        },
        body,
        &signed_in(),
    )
}

#[test]
fn an_ndjson_answer_is_admitted_as_its_lines_in_order() {
    let (request, _) = fetched(start(Rows));
    assert_eq!(request.encoding, Encoding::Ndjson { max_lines: 3 });
    assert_eq!(
        serde_json::to_value(&request).unwrap()["encoding"],
        json!({"kind": "ndjson", "max_lines": 3})
    );
    let lines = done(streamed(
        200,
        b"{\"id\":1}\r\n\n  \n{\"id\":2}\n{\"__type\":\"summary\",\"count\":2}",
    ));
    assert_eq!(
        lines,
        json!([{"id": 1}, {"id": 2}, {"__type": "summary", "count": 2}])
    );
    assert_eq!(done(streamed(200, b"")), json!([]));
}

#[test]
fn a_malformed_or_overlong_ndjson_answer_is_refused() {
    let malformed = refusal(streamed(200, b"{\"id\":1}\n{\"id\":\n"));
    assert_eq!(malformed.code, RESPONSE_MALFORMED);
    assert_eq!(malformed.message, "The answer's line 2 is not JSON.");
    let overlong = refusal(streamed(200, b"1\n2\n3\n4\n"));
    assert_eq!(overlong.code, RESPONSE_TOO_LARGE);
    assert_eq!(overlong.message, "The answer exceeded its 3 line bound.");
}

#[test]
fn an_ndjson_reads_refusal_is_still_its_json_envelope() {
    let refused = refusal(streamed_as(
        403,
        "application/json",
        br#"{"error":{"code":"FORBIDDEN","message":"platform.admin required"}}"#,
    ));
    assert_eq!(refused.code, "FORBIDDEN");
    assert_eq!(refused.message, "platform.admin required");
}

#[test]
fn a_json_request_names_no_encoding() {
    let (request, _) = fetched(start(Maps3dConfig));
    assert_eq!(request.encoding, Encoding::Json);
    assert!(serde_json::to_value(&request)
        .unwrap()
        .get("encoding")
        .is_none());
}

#[test]
fn an_operator_never_reads_a_remote_address_or_control_characters() {
    let raw = format!(
        "service refused at https://internal.example/{} then\u{0007}\u{0001}stopped",
        "x".repeat(700)
    );
    let refused = refusal(answer(Maps3dConfig, 400, json!({ "error": raw })));
    assert_eq!(
        refused.message,
        "service refused at [remote service] then stopped"
    );
    assert_eq!(refused.technical.as_deref(), Some(raw.as_str()));
    let long = refusal(answer(Maps3dConfig, 400, json!({"error": "y".repeat(800)})));
    assert_eq!(long.message.chars().count(), 500);
}
