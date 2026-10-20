use super::*;
use crate::gateway_exchange::{resume, start, Facts, Observed, Step};
use serde_json::json;

fn streamed(lines: &str) -> Result<Ledger, Refusal> {
    let Step::Fetch { request, continuation } = start(DeletedTransformers { days: Some(90), limit: None }) else { panic!() };
    assert_eq!(request.body, Some(json!({"action": "query_table", "table": "deleted_transformers", "days": 90})));
    let observed = Observed::Response { status: 200, content_type: Some("application/x-ndjson".into()), retry_after: None };
    match resume(continuation, observed, lines.as_bytes(), &Facts::default()) {
        Step::Done(ledger) => Ok(ledger),
        Step::Refused(refusal) => Err(refusal),
        _ => panic!("expected an outcome"),
    }
}

#[test]
fn the_ledger_reads_rows_in_order_and_counts_from_its_summary() {
    let ledger = streamed("{\"transformer_name\":\"TX-2\"}\n{\"transformer_name\":\"TX-1\"}\n\n{\"__type\":\"summary\",\"count\":2}\n").unwrap();
    assert_eq!(ledger.rows.iter().map(|row| row["transformer_name"].as_str().unwrap()).collect::<Vec<_>>(), ["TX-2", "TX-1"]);
    assert_eq!(ledger.count, 2);
}

#[test]
fn a_stream_that_failed_is_refused_not_shown_short() {
    let refusal = streamed("{\"transformer_name\":\"TX-2\"}\n{\"__type\":\"summary\",\"status\":\"error\"}\n").unwrap_err();
    assert_eq!(refusal.code, FAILED);
    assert!(streamed("{\"transformer_name\":\n").is_err());
}
