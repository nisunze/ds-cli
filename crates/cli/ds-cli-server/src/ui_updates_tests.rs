use super::*;
use std::time::Instant;

fn done(component: UiComponent, outcome: UiConvergenceOutcome, generation: u64) -> UiConvergence {
    UiConvergence {
        schema: "ds.ui-module-convergence/v1",
        component,
        channel: "canary".into(),
        outcome,
        generation,
        release_version: Some("0.1.0-r5".into()),
        unit_sha256: Some("a".repeat(64)),
    }
}

#[test]
fn passes_follow_the_first_delay_and_interval_and_stop_promptly() {
    let stop = AtomicBool::new(false);
    let mut seen = Vec::new();
    let started = Instant::now();
    run(
        &stop,
        Duration::from_millis(20),
        Duration::from_millis(10),
        &mut |component| {
            seen.push(component);
            if seen.len() == 6 {
                stop.store(true, Ordering::Release);
            }
            Ok(done(component, UiConvergenceOutcome::Current, 1))
        },
        &mut |message| panic!("a current unit says nothing: {message}"),
    );
    assert_eq!(seen.len(), 6, "three passes over both units");
    assert_eq!(&seen[..2], &COMPONENTS);
    assert!(started.elapsed() >= Duration::from_millis(40));
    assert!(started.elapsed() < Duration::from_secs(5));
}

#[test]
fn a_stopped_host_runs_no_pass() {
    let stop = AtomicBool::new(true);
    run(
        &stop,
        Duration::from_secs(3600),
        Duration::from_secs(3600),
        &mut |_| panic!("no pass after the host stopped"),
        &mut |_| {},
    );
    assert!(wait(&AtomicBool::new(false), Duration::ZERO));
    assert!(!wait(&stop, Duration::ZERO));
}

#[test]
fn an_update_is_said_and_a_repeated_failure_is_said_once_until_it_clears() {
    let mut said = [None, None];
    let mut lines = Vec::new();
    let mut outcomes = vec![
        Err("module_release_unavailable: offline".to_owned()),
        Ok(done(
            UiComponent::CommandKernel,
            UiConvergenceOutcome::Updated,
            6,
        )),
        Err("module_release_unavailable: offline".to_owned()),
        Ok(done(
            UiComponent::CommandKernel,
            UiConvergenceOutcome::Current,
            6,
        )),
        Ok(done(
            UiComponent::Interactions,
            UiConvergenceOutcome::Current,
            1,
        )),
        Err("module_update_busy: another update owns this UI component".to_owned()),
    ]
    .into_iter();
    for _ in 0..3 {
        pass(&mut |_| outcomes.next().unwrap(), &mut said, &mut |line| {
            lines.push(line.to_owned())
        });
    }
    assert_eq!(lines.len(), 3, "{lines:?}");
    assert!(
        lines[0].starts_with(
            "interactions UI unit update did not complete (module_release_unavailable"
        )
    );
    assert!(lines[1].starts_with("command-kernel UI unit is now 0.1.0-r5 (generation 6)"));
    assert!(
        lines[2].starts_with("command-kernel UI unit update did not complete (module_update_busy")
    );
    assert_eq!(said[0], None);
}
