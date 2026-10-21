use super::*;
use crate::gateway_exchange::{resume, start, Facts, Observed, Step};
use serde_json::json;

fn head_json(variant: &str) -> Value {
    json!({"work_id": "work_1", "engine": "solar", "operation": "report-kigali", "variant": variant,
        "input_base_fingerprint": "a".repeat(64), "head_revision": 3, "updated_at": "2026-10-19T08:00:00Z",
        "outputs": [{"output_id": "summary", "format": "pdf", "content_type": "application/pdf",
            "sha256": "b".repeat(64), "size_bytes": 4}]})
}

fn identity() -> HeadIdentity {
    HeadIdentity {
        project_id: "it_rwanda".into(),
        engine: "solar".into(),
        operation: "report-kigali".into(),
        variant: "default".into(),
    }
}

fn output() -> OutputRef {
    OutputRef { head: identity(), output_id: "summary".into() }
}

fn answered<C: Exchange>(call: C, status: u16, body: Value) -> Result<C::Output, Refusal> {
    let Step::Fetch { continuation, .. } = start(call) else { panic!("expected a fetch") };
    let observed = Observed::Response { status, content_type: Some("application/json".into()), retry_after: None };
    match resume(continuation, observed, body.to_string().as_bytes(), &Facts::default()) {
        Step::Done(value) => Ok(value),
        Step::Refused(refusal) => Err(refusal),
        _ => panic!("expected an outcome"),
    }
}

fn download_answer(url: &str) -> Value {
    json!({"success": true, "data": {"head": head_json("default"), "output": head_json("default")["outputs"][0],
        "download_url": url, "expires_at": "2026-10-20T09:00:00Z", "file_name": "summary.pdf"}})
}

#[test]
fn the_three_reads_name_only_their_selectors() {
    let Step::Fetch { request, .. } = start(ListHeads { project_id: "it_rwanda".into(), limit: Some(50), cursor: Some(" c ".into()) }) else { panic!() };
    assert_eq!(request.body, Some(json!({"action": "list", "project_id": "it_rwanda", "limit": 50, "cursor": "c"})));
    let Step::Fetch { request, .. } = start(ReadHead(identity())) else { panic!() };
    assert_eq!(request.body, Some(json!({"action": "read", "project_id": "it_rwanda", "engine": "solar",
        "operation": "report-kigali", "variant": "default"})));
    let Step::Fetch { request, .. } = start(ResolveDownload(output())) else { panic!() };
    assert_eq!(request.body, Some(json!({"action": "download", "project_id": "it_rwanda", "engine": "solar",
        "operation": "report-kigali", "variant": "default", "output_id": "summary"})));
    assert!(matches!(start(ListHeads { project_id: "it_rwanda".into(), limit: Some(101), cursor: None }),
        Step::Refused(refusal) if refusal.code == INPUT_INVALID));
    let mut stranger = identity();
    stranger.engine = "python".into();
    assert!(matches!(start(ReadHead(stranger)), Step::Refused(refusal) if refusal.code == INPUT_INVALID));
}

#[test]
fn a_page_is_admitted_whole_or_refused() {
    let page = answered(
        ListHeads { project_id: "it_rwanda".into(), limit: None, cursor: None },
        200,
        json!({"success": true, "data": {"heads": [head_json("default")], "has_more": true, "next_cursor": "n", "limit": 25}}),
    )
    .unwrap();
    assert_eq!((page.heads.len(), page.has_more, page.next_cursor.as_deref(), page.limit), (1, true, Some("n"), 25));
    let mut broken = head_json("default");
    broken["head_revision"] = json!(0);
    let refused = answered(
        ListHeads { project_id: "it_rwanda".into(), limit: None, cursor: None },
        200,
        json!({"data": {"heads": [broken], "has_more": false, "limit": 25}}),
    );
    assert_eq!(refused.unwrap_err().code, INVALID);
    let cursorless = answered(
        ListHeads { project_id: "it_rwanda".into(), limit: None, cursor: None },
        200,
        json!({"data": {"heads": [], "has_more": true, "limit": 25}}),
    );
    assert_eq!(cursorless.unwrap_err().code, INVALID);
}

#[test]
fn a_head_never_published_is_told_apart_from_a_refusal() {
    let missing = answered(ReadHead(identity()), 404, json!({"error": "Compute artifact head or output not found"})).unwrap_err();
    assert!(unpublished(&missing));
    let forbidden = answered(ReadHead(identity()), 403, json!({"error": "no"})).unwrap_err();
    assert!(!unpublished(&forbidden));
    let crossed = answered(ReadHead(identity()), 200, json!({"data": head_json("other")})).unwrap_err();
    assert_eq!(crossed.code, MISMATCH);
}

#[test]
fn a_download_is_generation_pinned_and_answers_the_output_asked_for() {
    let pinned = "https://storage.googleapis.com/b/o.pdf?generation=17&X-Goog-Signature=s";
    let download = answered(ResolveDownload(output()), 200, download_answer(pinned)).unwrap();
    assert_eq!(download.file_name, "summary.pdf");
    assert_eq!(download.output.size_bytes, 4);
    for unpinned in ["https://storage.googleapis.com/b/o.pdf", "http://x/o?generation=1", "https://x/o?generation=01"] {
        assert_eq!(answered(ResolveDownload(output()), 200, download_answer(unpinned)).unwrap_err().code, INVALID, "{unpinned}");
    }
    let mut other = output();
    other.output_id = "appendix".into();
    assert!(answered(ResolveDownload(other), 200, download_answer(pinned)).is_err());
}

#[test]
fn what_a_caller_holds_fences_the_read() {
    let download = answered(ResolveDownload(output()), 200,
        download_answer("https://storage.googleapis.com/b/o.pdf?generation=17")).unwrap();
    assert_eq!(admit_read(&download, &Expected::default(), 4), Ok(()));
    assert_eq!(admit_read(&download, &Expected::default(), 3).unwrap_err().code, crate::gateway_exchange::RESPONSE_TOO_LARGE);
    let stale = Expected { sha256: Some("c".repeat(64)), ..Expected::default() };
    assert_eq!(admit_read(&download, &stale, 4).unwrap_err().code, MISMATCH);
    let renamed = Expected { file_name: Some("other.pdf".into()), ..Expected::default() };
    assert_eq!(admit_read(&download, &renamed, 4).unwrap_err().code, MISMATCH);
}

#[test]
fn a_locator_names_one_output() {
    let locator = Locator::parse("compute-artifact://v1/it_rwanda/solar/report-kigali/default/summary/Kigali%20report.pdf").unwrap();
    assert_eq!(locator.output, output());
    assert_eq!(locator.file_name, "Kigali report.pdf");
    for bad in [
        "compute-artifact://v2/it_rwanda/solar/report-kigali/default/summary/a.pdf",
        "compute-artifact://v1/it_rwanda/solar/report-kigali/default/summary",
        "compute-artifact://v1/it_rwanda/python/report-kigali/default/summary/a.pdf",
        "compute-artifact://v1/it_rwanda/solar/report-kigali/default/summary/a.pdf?x=1",
        "compute-artifact://v1/it_rwanda/solar/report-kigali/default/summary/a%2Fb.pdf",
        "https://example.com/a.pdf",
    ] {
        assert!(Locator::parse(bad).is_none(), "{bad}");
    }
}
