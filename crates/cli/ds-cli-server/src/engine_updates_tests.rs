use super::*;
use std::time::Instant;

fn done(outcome: EngineConvergenceOutcome, generation: u64) -> EngineConvergence {
    EngineConvergence {
        schema: "ds.engine-unit-convergence/v1",
        component: COMPONENT,
        channel: "canary".into(),
        outcome,
        generation,
        release_version: Some("0.1.0-r2".into()),
        unit_sha256: Some("a".repeat(64)),
    }
}

#[test]
fn passes_follow_the_first_delay_and_interval_and_stop_promptly() {
    let stop = AtomicBool::new(false);
    let mut passes = 0;
    let started = Instant::now();
    run(
        &stop,
        Duration::from_millis(20),
        Duration::from_millis(10),
        &mut || {
            passes += 1;
            if passes == 3 {
                stop.store(true, Ordering::Release);
            }
            Ok(done(EngineConvergenceOutcome::Current, 1))
        },
        &mut |message| panic!("a current unit says nothing: {message}"),
    );
    assert_eq!(passes, 3);
    assert!(started.elapsed() >= Duration::from_millis(40));
    assert!(started.elapsed() < Duration::from_secs(5));
    let stopped = AtomicBool::new(true);
    run(
        &stopped,
        Duration::from_secs(60),
        Duration::from_secs(60),
        &mut || panic!("a stopped host runs no pass"),
        &mut |_| {},
    );
}

#[test]
fn an_update_is_said_once_and_a_failure_once_until_it_changes_or_clears() {
    let mut outcomes = vec![
        Ok(done(EngineConvergenceOutcome::Updated, 2)),
        Err("module_release_unavailable: offline".to_owned()),
        Err("module_release_unavailable: offline".to_owned()),
        Err("module_update_busy: another update owns this engine unit".to_owned()),
        Ok(done(EngineConvergenceOutcome::Absent, 0)),
        Err("module_update_busy: another update owns this engine unit".to_owned()),
    ]
    .into_iter();
    let mut said = None;
    let mut lines = Vec::new();
    for _ in 0..6 {
        pass(&mut || outcomes.next().unwrap(), &mut said, &mut |line| {
            lines.push(line.to_owned())
        });
    }
    assert_eq!(lines.len(), 4);
    assert!(lines[0].contains("0.1.0-r2 (generation 2)"));
    assert!(lines[1].contains("offline"));
    assert!(lines[2].contains("module_update_busy"));
    assert!(lines[3].contains("module_update_busy"), "said again after it cleared");
}
