use super::*;
use ds_solar_contracts::ProgressPhase;

#[test]
fn cloned_sinks_deliver_exact_events_in_order_and_return_independent_snapshots() {
    let sink = CollectingProgressSink::new();
    let clone = sink.clone();
    let started = ProgressEvent::run(ProgressPhase::RunStarted, "accepted");
    let finished = ProgressEvent::run(ProgressPhase::RunFinished, "settled");
    sink.emit(started.clone());
    let mut snapshot = clone.events();
    clone.emit(finished.clone());
    snapshot.clear();
    assert_eq!(sink.events(), vec![started, finished]);
}
