use super::*;

fn scope() -> Scope {
    Scope {
        account_id: "account-a".into(),
        project_id: "project-a".into(),
    }
}
fn row() -> Row {
    Row {
        path: "survey/a.jpg".into(),
        surface: "survey-media".into(),
        owner_ref: "entry-a".into(),
        content_type: "image/jpeg".into(),
        size: 4,
        status: Status::Pending,
        attempts: 0,
        next_attempt_at: 0,
        last_error: None,
        created_at: 1,
        updated_at: 1,
        scope: Some(scope()),
        lease: 0,
        generation: "stash-a".into(),
        expected_generation: None,
        companion: None,
        revision: None,
        supersedes: Vec::new(),
        source: None,
    }
}
fn drive(row: Row, now: u64) -> Reply {
    apply(
        now,
        Some(row),
        Event::Drive {
            online: true,
            active_uploads: 0,
            bytes_present: true,
            scope: scope(),
        },
    )
    .unwrap()
}

#[test]
fn dispatch_is_bounded_and_leased() {
    let reply = drive(row(), 10);
    assert_eq!(reply.row.as_ref().unwrap().status, Status::Uploading);
    assert_eq!(reply.row.as_ref().unwrap().attempts, 1);
    assert_eq!(
        reply.actions,
        vec![Action::Dispatch {
            path: "survey/a.jpg".into(),
            content_type: "image/jpeg".into(),
            generation: "stash-a".into(),
            expected_generation: None,
            lease: 1,
            targets: vec![UploadTarget {
                path: "survey/a.jpg".into(),
                content_type: "image/jpeg".into(),
                size: 4,
                expected_generation: None,
                revision: None,
                supersedes: Vec::new(),
                source: None,
                quarter_turns: 0,
            }],
        }]
    );
    let capped = apply(
        10,
        Some(row()),
        Event::Drive {
            online: true,
            active_uploads: MAX_CONCURRENT_UPLOADS,
            bytes_present: true,
            scope: scope(),
        },
    )
    .unwrap();
    assert!(capped.actions.is_empty());
}

#[test]
fn companion_has_one_canonical_ordered_plan_and_cleanup_unpins_both_bytes() {
    let mut state = row();
    state.companion = Some(Companion {
        path: "survey/a_thunder.jpeg".into(),
        content_type: "image/jpeg".into(),
        size: 3,
        expected_generation: None,
        quarter_turns: 0,
    });
    let started = drive(state, 10);
    let Action::Dispatch { targets, .. } = &started.actions[0] else {
        panic!("dispatch expected")
    };
    assert_eq!(
        targets
            .iter()
            .map(|target| target.path.as_str())
            .collect::<Vec<_>>(),
        vec!["survey/a.jpg", "survey/a_thunder.jpeg"]
    );
    let removed = apply(
        11,
        started.row,
        Event::Result {
            scope: scope(),
            generation: "stash-a".into(),
            lease: 1,
            outcome: TransportOutcome::Success,
        },
    )
    .unwrap();
    assert!(removed.row.is_none());
    assert_eq!(
        removed
            .actions
            .iter()
            .filter(|action| matches!(action, Action::UnpinBlob { .. }))
            .count(),
        2
    );
}

#[test]
fn invalid_companion_cannot_admit_or_create_an_upload_plan() {
    let mut state = row();
    state.companion = Some(Companion {
        path: "survey/a-thumb.jpg".into(),
        content_type: "image/png".into(),
        size: 0,
        expected_generation: None,
        quarter_turns: 0,
    });
    assert!(apply(1, Some(state.clone()), Event::Recover).is_err());
    assert!(upload_plan(&state).is_err());
    assert_eq!(
        thumbnail_path_for("folder/field.capture.png").unwrap(),
        "folder/field.capture_thunder.jpeg"
    );
    assert_eq!(
        thumbnail_path_for("folder/field_thunder.JPEG").unwrap(),
        "folder/field_thunder.JPEG"
    );
    let mut same_path = row();
    same_path.companion = Some(Companion {
        path: same_path.path.clone(),
        content_type: "image/jpeg".into(),
        size: 3,
        expected_generation: None,
        quarter_turns: 0,
    });
    assert!(apply(1, Some(same_path), Event::Recover).is_err());
}

#[test]
fn restart_recovers_without_spending_an_attempt() {
    let mut state = row();
    state.status = Status::Uploading;
    state.attempts = 3;
    state.lease = 9;
    let reply = apply(50, Some(state), Event::Recover).unwrap();
    let recovered = reply.row.unwrap();
    assert_eq!(recovered.status, Status::Pending);
    assert_eq!(recovered.attempts, 3);
}

#[test]
fn recovered_tenth_attempt_never_dispatches_an_eleventh() {
    let mut state = row();
    state.status = Status::Uploading;
    state.attempts = MAX_ATTEMPTS;
    let recovered = apply(50, Some(state), Event::Recover).unwrap().row.unwrap();
    let reply = drive(recovered, 51);
    assert!(reply.actions.is_empty());
    assert_eq!(reply.row.unwrap().status, Status::Pending);
}

#[test]
fn expected_generation_is_opaque_but_must_be_positive_decimal() {
    let mut invalid = row();
    invalid.expected_generation = Some("not-a-generation".into());
    assert!(apply(1, Some(invalid), Event::Recover).is_err());
    let mut zero = row();
    zero.expected_generation = Some("0".into());
    assert!(apply(1, Some(zero), Event::Recover).is_err());
}

#[test]
fn transient_failure_uses_response_policy_and_deadline() {
    let started = drive(row(), 100).row.unwrap();
    let reply = apply(
        100,
        Some(started),
        Event::Result {
            scope: scope(),
            generation: "stash-a".into(),
            lease: 1,
            outcome: TransportOutcome::TransportFailure {
                cause: None,
                status: Some(503),
                error_code: None,
                timed_out: false,
                retry_after: None,
                has_session: true,
                offline_mode: false,
                message: Some("unavailable".into()),
            },
        },
    )
    .unwrap();
    let state = reply.row.unwrap();
    assert_eq!(state.status, Status::Pending);
    assert_eq!(state.next_attempt_at, 1_900);
    assert_eq!(reply.actions, vec![Action::WakeAt { at: 1_900 }]);
}

#[test]
fn tenth_failure_is_terminal_and_manual_retry_resets_budget() {
    let mut state = row();
    state.status = Status::Uploading;
    state.attempts = 10;
    state.lease = 4;
    let failed = apply(
        1,
        Some(state),
        Event::Result {
            scope: scope(),
            generation: "stash-a".into(),
            lease: 4,
            outcome: TransportOutcome::TransportFailure {
                cause: None,
                status: Some(503),
                error_code: None,
                timed_out: false,
                retry_after: None,
                has_session: true,
                offline_mode: false,
                message: None,
            },
        },
    )
    .unwrap()
    .row
    .unwrap();
    assert_eq!(failed.status, Status::Failed);
    let retried = apply(2, Some(failed), Event::ManualRetry { scope: scope() })
        .unwrap()
        .row
        .unwrap();
    assert_eq!((retried.status, retried.attempts), (Status::Pending, 0));
}

#[test]
fn superseding_storage_precondition_retries_but_corrupt_bytes_do_not() {
    for (cause, expected) in [
        (Cause::HttpStatus { status: 412 }, Status::Pending),
        (Cause::DigestMismatch, Status::Failed),
        (Cause::LengthMismatch, Status::Failed),
        (Cause::HttpStatus { status: 409 }, Status::Failed),
    ] {
        let mut started = drive(row(), 0).row.unwrap();
        started.revision = Some(started.generation.clone());
        let result = apply(
            0,
            Some(started),
            Event::Result {
                scope: scope(),
                generation: "stash-a".into(),
                lease: 1,
                outcome: TransportOutcome::TransportFailure {
                    cause: Some(cause),
                    status: None,
                    error_code: None,
                    timed_out: false,
                    retry_after: None,
                    has_session: true,
                    offline_mode: false,
                    message: None,
                },
            },
        )
        .unwrap()
        .row
        .unwrap();
        assert_eq!(result.status, expected);
    }
}

#[test]
fn inconclusive_308_is_normalized_by_the_transport_and_412_is_terminal() {
    let started = drive(row(), 0).row.unwrap();
    let retry = apply(
        0,
        Some(started.clone()),
        Event::Result {
            scope: scope(),
            generation: "stash-a".into(),
            lease: 1,
            // The governed transport reports an incomplete 308 as this
            // no-status fact; response_policy then owns its retry timing.
            outcome: TransportOutcome::TransportFailure {
                cause: None,
                status: None,
                error_code: Some("UPLOAD_INCONCLUSIVE".into()),
                timed_out: false,
                retry_after: None,
                has_session: true,
                offline_mode: false,
                message: None,
            },
        },
    )
    .unwrap()
    .row
    .unwrap();
    assert_eq!(retry.status, Status::Pending);
    let conflict = apply(
        0,
        Some(started),
        Event::Result {
            scope: scope(),
            generation: "stash-a".into(),
            lease: 1,
            outcome: TransportOutcome::TransportFailure {
                cause: None,
                status: Some(412),
                error_code: None,
                timed_out: false,
                retry_after: None,
                has_session: true,
                offline_mode: false,
                message: None,
            },
        },
    )
    .unwrap()
    .row
    .unwrap();
    assert_eq!(conflict.status, Status::Failed);
}

#[test]
fn typed_transfer_causes_share_response_policy_retry_and_terminal_rules() {
    for cause in [
        Cause::HttpStatus { status: 503 },
        Cause::Timeout {
            phase: Phase::Response,
        },
        Cause::SessionExpired,
        Cause::BlockedByHostPolicy,
    ] {
        let started = drive(row(), 0).row.unwrap();
        let reply = apply(
            0,
            Some(started),
            Event::Result {
                scope: scope(),
                generation: "stash-a".into(),
                lease: 1,
                outcome: TransportOutcome::TransportFailure {
                    cause: Some(cause),
                    status: None,
                    error_code: None,
                    timed_out: false,
                    retry_after: None,
                    has_session: true,
                    offline_mode: false,
                    message: None,
                },
            },
        )
        .unwrap();
        assert_eq!(reply.row.unwrap().status, Status::Pending);
    }
    let started = drive(row(), 0).row.unwrap();
    let reply = apply(
        0,
        Some(started),
        Event::Result {
            scope: scope(),
            generation: "stash-a".into(),
            lease: 1,
            outcome: TransportOutcome::TransportFailure {
                cause: Some(Cause::DigestMismatch),
                status: None,
                error_code: None,
                timed_out: false,
                retry_after: None,
                has_session: true,
                offline_mode: false,
                message: None,
            },
        },
    )
    .unwrap();
    assert_eq!(reply.row.unwrap().status, Status::Failed);
}

#[test]
fn missing_bytes_and_scope_change_never_dispatch() {
    let missing = apply(
        1,
        Some(row()),
        Event::Drive {
            online: true,
            active_uploads: 0,
            bytes_present: false,
            scope: scope(),
        },
    )
    .unwrap()
    .row
    .unwrap();
    assert_eq!(missing.status, Status::Failed);
    let other = Scope {
        account_id: "account-b".into(),
        project_id: "project-a".into(),
    };
    let blocked = apply(
        2,
        Some(row()),
        Event::Drive {
            online: true,
            active_uploads: 0,
            bytes_present: true,
            scope: other,
        },
    )
    .unwrap()
    .row
    .unwrap();
    assert_eq!(blocked.status, Status::Blocked);
}

#[test]
fn late_completion_cannot_settle_a_new_lease() {
    let mut state = row();
    state.status = Status::Uploading;
    state.lease = 2;
    let reply = apply(
        1,
        Some(state),
        Event::Result {
            scope: scope(),
            generation: "stash-a".into(),
            lease: 1,
            outcome: TransportOutcome::Success,
        },
    )
    .unwrap();
    assert!(reply.row.is_some());
    assert!(reply.actions.is_empty());
}

#[test]
fn unscoped_row_is_blocked_and_the_current_account_cannot_claim_it() {
    let mut state = row();
    state.scope = None;
    let blocked = drive(state, 1).row.unwrap();
    assert_eq!(blocked.status, Status::Blocked);
    let retried = apply(2, Some(blocked), Event::ManualRetry { scope: scope() })
        .unwrap()
        .row
        .unwrap();
    assert_eq!(retried.status, Status::Blocked);
}
