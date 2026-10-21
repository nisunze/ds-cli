use super::*;

fn room(name: &str, version: i64) -> Room {
    let mut room = Room {
        transformer: name.into(),
        version: Some(version),
        content_digest: None,
        layers: BTreeMap::from([(
            "tr".to_string(),
            json!({"type": "FeatureCollection", "features": [{"type": "Feature"}]}),
        )]),
    };
    room.content_digest =
        Some(ds_command_kernel::report_export::jcs::layers_content_digest(&room.layers).unwrap());
    room
}

fn consistent_room(name: &str, version: i64) -> Room {
    room(name, version)
}

fn stale_projection(name: &str, version: i64) -> Room {
    let mut room = room(name, version);
    room.content_digest = Some("0".repeat(64));
    room
}

fn unreachable() -> Failure {
    Failure::unavailable(
        "device_auth_transient",
        "the fixed DS device authorization endpoint is temporarily unreachable",
    )
}

fn names(list: &[&str]) -> Vec<String> {
    list.iter().map(|name| (*name).to_owned()).collect()
}

/// The field curve with its pauses removed: the same attempts, no wait.
fn quick_link() -> Link {
    Link::with_schedule(WeakNetwork::new(&[std::time::Duration::ZERO; 3]))
}

fn blink() -> Failure {
    Failure::unavailable(
        super::super::AUTH_TRANSIENT.code,
        "the service answered 503",
    )
}

/// Acceptance A: one early 503 on a read is a blink, not an outage. It
/// is asked again under the shared curve, the service's answer is used,
/// and nothing latches — so no later read is answered from the hold.
#[test]
fn an_early_blink_is_retried_and_never_latches_an_outage() {
    let mut link = quick_link();
    let mut attempts = 0;
    let read = link.read(|| {
        attempts += 1;
        if attempts == 1 {
            Err(blink())
        } else {
            Ok("inventory")
        }
    });
    assert_eq!(read.unwrap(), Some("inventory"));
    assert_eq!(attempts, 2);
    assert_eq!(link.unreachable(), None);
    assert_eq!(link.read(|| Ok(1)).unwrap(), Some(1));
}

/// Only a service still unreachable after the whole curve latches, and
/// then no later read spends the curve again.
#[test]
fn only_an_outage_that_outlives_the_curve_latches() {
    let mut link = quick_link();
    let mut attempts = 0;
    let read = link.read(|| {
        attempts += 1;
        Err::<(), _>(blink())
    });
    assert!(read.unwrap().is_none());
    assert_eq!(attempts, 1 + WeakNetwork::FIELD.max_retries());
    assert!(link.unreachable().unwrap().contains("503"));
    assert!(
        link.read(|| -> Result<(), Failure> { panic!("latched") })
            .unwrap()
            .is_none()
    );
}

/// Acceptance B: the service answering that it could not read ONE
/// transformer is that transformer's row. It latches no outage, and every
/// later room is still asked for.
#[test]
fn one_unreadable_transformer_is_its_row_not_an_outage() {
    let dir = tempfile::tempdir().unwrap();
    let hold = Hold::at(dir.path().to_path_buf(), "uid-1", "project-1");
    let mut link = quick_link();
    let mut rooms =
        Rooms::plan(hold, &names(&["t1", "t2", "t3"]), Heads::Read(Vec::new())).unwrap();
    let failure = rooms
        .room("t1", &mut link, || {
            Err(Failure::unavailable(
                super::super::AUTH_TRANSIENT.code,
                "the transformer context service could not read the selected transformer",
            )
            .detail(json!({"service_code": TRANSFORMER_READ_FAILED})))
        })
        .unwrap_err();
    assert_eq!(failure.code(), super::super::AUTH_TRANSIENT.code);
    assert_eq!(
        failure.detail_value().unwrap()["service_code"],
        TRANSFORMER_READ_FAILED
    );
    assert_eq!(link.unreachable(), None);
    for name in ["t2", "t3"] {
        rooms.room(name, &mut link, || Ok(room(name, 1))).unwrap();
        assert_eq!(rooms.source(name), Some("fetched"));
    }
    let receipt = rooms.receipt();
    assert_eq!(receipt["rooms_fetched"], 2);
    assert_eq!(receipt["not_held"], json!([]));
}

/// A project-wide batch reuses current held rooms, fetches the moved one,
/// and latches an outage once: the rooms after it come from the hold or
/// are named as failures, never re-asked under the retry curve.
#[test]
fn a_batch_reuses_current_rooms_and_latches_an_outage_once() {
    let dir = tempfile::tempdir().unwrap();
    let hold = || Hold::at(dir.path().to_path_buf(), "uid-1", "project-1");
    hold().hold_room(&consistent_room("t1", 3)).unwrap();
    hold().hold_room(&consistent_room("t3", 1)).unwrap();
    let heads = vec![
        json!({"name": "t1", "version": 3}),
        json!({"name": "t2", "version": 2}),
        json!({"name": "t3", "version": 1}),
        json!({"name": "t4", "version": 5}),
    ];
    let mut reads = Vec::new();
    let mut seen = Vec::new();
    let acquisition = acquire_rooms(
        hold(),
        &names(&["t1", "t2", "t3", "t4"]),
        Some(&heads),
        quick_link(),
        |name| {
            reads.push(name.to_owned());
            Err(unreachable())
        },
        |room, source| {
            seen.push((room.transformer, source));
            Ok(())
        },
    )
    .unwrap();
    assert_eq!(
        seen,
        vec![("t1".to_owned(), "held"), ("t3".to_owned(), "held")]
    );
    assert!(
        !reads.is_empty() && reads.iter().all(|name| name == "t2"),
        "the outage latched on t2 and t4 was never asked: {reads:?}"
    );
    let failed: Vec<&str> = acquisition
        .failures
        .iter()
        .map(|(name, _)| name.as_str())
        .collect();
    assert_eq!(failed, vec!["t2", "t4"]);
    assert!(acquisition.unreachable.is_some());
    assert_eq!(acquisition.receipt["reused"], 2);
}

/// An explicit held read makes no service request at all.
#[test]
fn a_held_batch_reads_no_service() {
    let dir = tempfile::tempdir().unwrap();
    let hold = || Hold::at(dir.path().to_path_buf(), "uid-1", "project-1");
    hold().hold_room(&consistent_room("t1", 3)).unwrap();
    let mut count = 0;
    let acquisition = acquire_rooms(
        hold(),
        &names(&["t1", "t2"]),
        None,
        quick_link(),
        |_| panic!("a held read asked the service"),
        |_, source| {
            assert_eq!(source, "held");
            count += 1;
            Ok(())
        },
    )
    .unwrap();
    assert_eq!(count, 1);
    assert_eq!(acquisition.failures.len(), 1);
    assert_eq!(acquisition.failures[0].1.code(), ROOM_NOT_HELD.code);
}

/// The acceptance: once every room is held and the heads have not moved,
/// a run reads no room from the service at all.
#[test]
fn a_warm_run_fetches_no_room() {
    let dir = tempfile::tempdir().unwrap();
    let hold = || Hold::at(dir.path().to_path_buf(), "uid-1", "project-1");
    let batch = names(&["t1", "t2"]);
    let mut link = quick_link();

    let mut cold = Rooms::plan(hold(), &batch, Heads::Read(Vec::new())).unwrap();
    for name in &batch {
        cold.room(name, &mut link, || Ok(room(name, 3))).unwrap();
    }
    assert_eq!(cold.receipt()["rooms_fetched"], 2);

    let heads = vec![
        json!({"name": "t1", "version": 3}),
        json!({"name": "t2", "version": 3}),
    ];
    let mut warm = Rooms::plan(hold(), &batch, Heads::Read(heads)).unwrap();
    for name in &batch {
        let held = warm
            .room(name, &mut link, || panic!("a warm run read {name}"))
            .unwrap();
        assert_eq!(held, room(name, 3));
        assert_eq!(warm.source(name), Some("held"));
    }
    assert_eq!(warm.receipt()["rooms_fetched"], 0);
    assert_eq!(warm.receipt()["reused"], 2);
}

#[test]
fn valid_saved_digest_is_reused_without_a_room_read() {
    let dir = tempfile::tempdir().unwrap();
    let hold = || Hold::at(dir.path().to_path_buf(), "uid-1", "project-1");
    let expected = consistent_room("t1", 8);
    hold().hold_room(&expected).unwrap();
    let mut rooms = Rooms::plan(
        hold(),
        &names(&["t1"]),
        Heads::Read(vec![json!({"name": "t1", "version": 8})]),
    )
    .unwrap();
    let mut link = quick_link();
    let actual = rooms
        .room("t1", &mut link, || {
            panic!("a valid held room was refetched")
        })
        .unwrap();
    assert_eq!(actual, expected);
    assert_eq!(rooms.receipt()["reused"], 1);
    assert_eq!(rooms.receipt()["rooms_fetched"], 0);
}

#[test]
fn an_invalid_projected_hold_is_refetched_and_replaced_online() {
    let dir = tempfile::tempdir().unwrap();
    let hold = || Hold::at(dir.path().to_path_buf(), "uid-1", "project-1");
    let old = stale_projection("t1", 8);
    hold().hold_room(&old).unwrap();
    let fresh = consistent_room("t1", 8);
    let mut rooms = Rooms::plan(
        hold(),
        &names(&["t1"]),
        Heads::Read(vec![json!({"name": "t1", "version": 8})]),
    )
    .unwrap();
    let mut link = quick_link();
    let mut reads = 0;
    let actual = rooms
        .room("t1", &mut link, || {
            reads += 1;
            Ok(fresh.clone())
        })
        .unwrap();
    assert_eq!(reads, 1);
    assert_eq!(actual, fresh);
    assert_eq!(
        room_hold::room(&hold().root, &hold().scope, "t1").unwrap(),
        Some(fresh)
    );
    assert_eq!(rooms.receipt()["reused"], 0);
    assert_eq!(rooms.receipt()["rooms_fetched"], 1);
}

#[test]
fn an_invalid_projected_hold_is_named_and_preserved_offline() {
    let dir = tempfile::tempdir().unwrap();
    let hold = || Hold::at(dir.path().to_path_buf(), "uid-1", "project-1");
    let old = stale_projection("t1", 8);
    hold().hold_room(&old).unwrap();
    let mut link = quick_link();
    assert!(link.read(|| Err::<(), _>(unreachable())).unwrap().is_none());
    let mut rooms = Rooms::plan(hold(), &names(&["t1"]), Heads::Unreachable).unwrap();
    let failure = rooms
        .room("t1", &mut link, || {
            panic!("an offline room read was attempted")
        })
        .unwrap_err();
    assert_eq!(failure.code(), ROOM_NOT_HELD.code);
    assert!(failure.message().contains("t1"));
    assert!(failure.message().contains("content digest"));
    assert_eq!(
        room_hold::room(&hold().root, &hold().scope, "t1").unwrap(),
        Some(old)
    );
    assert_eq!(rooms.receipt()["reused"], 0);
    assert_eq!(rooms.receipt()["rooms_fetched"], 0);
    assert_eq!(rooms.receipt()["not_held"], json!(["t1"]));
}

#[test]
fn a_digestless_legacy_hold_refreshes_once_when_online() {
    let dir = tempfile::tempdir().unwrap();
    let hold = || Hold::at(dir.path().to_path_buf(), "uid-1", "project-1");
    let mut legacy = room("t1", 8);
    legacy.content_digest = None;
    hold().hold_room(&legacy).unwrap();
    let fresh = consistent_room("t1", 8);
    let mut rooms = Rooms::plan(
        hold(),
        &names(&["t1"]),
        Heads::Read(vec![json!({"name": "t1", "version": 8})]),
    )
    .unwrap();
    let mut link = quick_link();
    let mut reads = 0;
    assert_eq!(
        rooms
            .room("t1", &mut link, || {
                reads += 1;
                Ok(fresh.clone())
            })
            .unwrap(),
        fresh
    );
    assert_eq!(reads, 1);
    assert_eq!(rooms.receipt()["rooms_fetched"], 1);
}

#[test]
fn an_inconsistent_fresh_room_is_refused_without_replacing_the_hold() {
    let dir = tempfile::tempdir().unwrap();
    let hold = || Hold::at(dir.path().to_path_buf(), "uid-1", "project-1");
    let old = stale_projection("t1", 8);
    hold().hold_room(&old).unwrap();
    let mut rooms = Rooms::plan(
        hold(),
        &names(&["t1"]),
        Heads::Read(vec![json!({"name": "t1", "version": 8})]),
    )
    .unwrap();
    let mut link = quick_link();
    let failure = rooms
        .room("t1", &mut link, || Ok(stale_projection("t1", 8)))
        .unwrap_err();
    assert_eq!(failure.code(), "report_inputs_invalid");
    assert!(failure.message().contains("t1"));
    assert_eq!(
        room_hold::room(&hold().root, &hold().scope, "t1").unwrap(),
        Some(old)
    );
    assert_eq!(rooms.receipt()["reused"], 0);
    assert_eq!(rooms.receipt()["rooms_fetched"], 0);
}

/// A moved head is read again; the service's answer replaces the copy.
#[test]
fn a_moved_head_reads_that_room_again_and_holds_it() {
    let dir = tempfile::tempdir().unwrap();
    let hold = || Hold::at(dir.path().to_path_buf(), "uid-1", "project-1");
    let mut link = quick_link();
    let mut cold = Rooms::plan(hold(), &names(&["t1"]), Heads::Unreachable).unwrap();
    cold.room("t1", &mut link, || Ok(room("t1", 3))).unwrap();

    let heads = vec![json!({"name": "t1", "version": 4})];
    let mut moved = Rooms::plan(hold(), &names(&["t1"]), Heads::Read(heads)).unwrap();
    let read = moved.room("t1", &mut link, || Ok(room("t1", 4))).unwrap();
    assert_eq!(read.version, Some(4));
    assert_eq!(moved.receipt()["rooms_fetched"], 1);
    let again = Rooms::plan(
        hold(),
        &names(&["t1"]),
        Heads::Read(vec![json!({"name": "t1", "version": 4})]),
    )
    .unwrap()
    .room("t1", &mut link, || panic!("settled"))
    .unwrap();
    assert_eq!(again.version, Some(4));
}

/// A refused heads read proves nothing current: every room is read again,
/// exactly as before rooms were held.
#[test]
fn refused_heads_read_every_room_again() {
    let dir = tempfile::tempdir().unwrap();
    let hold = || Hold::at(dir.path().to_path_buf(), "uid-1", "project-1");
    let mut link = quick_link();
    let mut cold = Rooms::plan(hold(), &names(&["t1"]), Heads::Unreachable).unwrap();
    cold.room("t1", &mut link, || Ok(room("t1", 3))).unwrap();
    let mut refused = Rooms::plan(
        hold(),
        &names(&["t1"]),
        Heads::Refused("auth_response_unreadable: bad".into()),
    )
    .unwrap();
    refused.room("t1", &mut link, || Ok(room("t1", 3))).unwrap();
    let receipt = refused.receipt();
    assert_eq!(receipt["rooms_fetched"], 1);
    assert_eq!(receipt["heads"], "refused: auth_response_unreadable: bad");
}

/// With no link, every held room prints and each one that is not held is
/// named; the service is asked once, never once per room.
#[test]
fn with_no_link_held_rooms_print_and_the_rest_are_named() {
    let dir = tempfile::tempdir().unwrap();
    let hold = || Hold::at(dir.path().to_path_buf(), "uid-1", "project-1");
    let mut link = quick_link();
    let mut cold = Rooms::plan(hold(), &names(&["t1"]), Heads::Unreachable).unwrap();
    cold.room("t1", &mut link, || Ok(room("t1", 3))).unwrap();

    let mut offline = quick_link();
    assert!(
        offline
            .read(|| Err::<(), _>(unreachable()))
            .unwrap()
            .is_none()
    );
    let mut rooms = Rooms::plan(hold(), &names(&["t1", "t2", "t3"]), Heads::Unreachable).unwrap();
    assert_eq!(
        rooms.room("t1", &mut offline, || panic!("held")).unwrap(),
        room("t1", 3)
    );
    for name in ["t2", "t3"] {
        let failure = rooms
            .room(name, &mut offline, || {
                panic!("the link is known to be down")
            })
            .unwrap_err();
        assert_eq!(failure.code(), ROOM_NOT_HELD.code);
        assert!(failure.message().contains(name), "{}", failure.message());
        assert!(
            failure
                .message()
                .contains(DEVICE_AUTH_TRANSIENT_REFUSAL.code)
        );
    }
    let receipt = rooms.receipt();
    assert_eq!(receipt["reused"], 1);
    assert_eq!(receipt["rooms_fetched"], 0);
    assert_eq!(receipt["not_held"], json!(["t2", "t3"]));
    assert_eq!(receipt["heads"], "unread");
}

/// Only an unreachable service falls back to the hold; the service's own
/// refusal is still the answer.
#[test]
fn a_refusal_is_not_an_outage() {
    let mut link = quick_link();
    let refused = link
        .read(|| Err::<(), _>(Failure::unauthorized("headless_signed_out", "signed out")))
        .unwrap_err();
    assert_eq!(refused.code(), "headless_signed_out");
    assert_eq!(link.unreachable(), None);
    assert_eq!(link.read(|| Ok(1)).unwrap(), Some(1));
}

#[test]
fn inputs_are_held_whole_and_refused_by_name_when_absent() {
    let dir = tempfile::tempdir().unwrap();
    let hold = Hold::at(dir.path().to_path_buf(), "uid-1", "project-1");
    let failure = hold
        .inputs(&format!("{}: down", DEVICE_AUTH_TRANSIENT_REFUSAL.code))
        .unwrap_err();
    assert_eq!(failure.code(), INPUTS_NOT_HELD.code);
    assert!(
        failure
            .message()
            .contains(DEVICE_AUTH_TRANSIENT_REFUSAL.code)
    );
    let inputs = Inputs {
        project_crs: None,
        printing_style_capture: None,
        transformer_natures: None,
        saved_analyses: BTreeMap::new(),
        rows: vec![Row {
            name: "t1".into(),
            kind: "transformer".into(),
            state: "active".into(),
            retired: false,
            reason: None,
        }],
        configuration: json!({"network_reporter_input_receipt": {"schema": "x"}}),
        setups: vec![json!({"id": "a3", "revision": 2, "layout": {}})],
        read_at: "2026-09-27T05:00:00Z".into(),
    };
    hold.hold_inputs(&inputs).unwrap();
    assert_eq!(hold.inputs("unused").unwrap(), inputs);
}

#[test]
fn a_requested_name_the_project_lacks_reads_missing() {
    let rows = vec![Row {
        name: "t1".into(),
        kind: "transformer".into(),
        state: "active".into(),
        retired: false,
        reason: None,
    }];
    let requested = TransformerSet::new(["t2".to_string(), "t1".to_string()]).unwrap();
    let selected = select(&rows, &requested);
    let states: Vec<(&str, &str)> = selected
        .iter()
        .map(|row| (row.name.as_str(), row.state.as_str()))
        .collect();
    assert!(states.contains(&("t1", "active")));
    assert!(states.contains(&("t2", "missing")));
    assert_eq!(select(&rows, &TransformerSet::default()), rows);
}

/// A print that publishes nothing reads no heads: a held room prints as held,
/// even one older than its head, and only an unheld room is read, once.
#[test]
fn a_held_first_print_reuses_held_rooms_and_reads_only_unheld_ones() {
    let dir = tempfile::tempdir().unwrap();
    let hold = || Hold::at(dir.path().to_path_buf(), "uid-1", "project-1");
    let mut link = quick_link();
    let mut cold = Rooms::plan(hold(), &names(&["t1"]), Heads::Unreachable).unwrap();
    cold.room("t1", &mut link, || Ok(room("t1", 3))).unwrap();

    let mut rooms = Rooms::plan(hold(), &names(&["t1", "t2"]), Heads::Held).unwrap();
    let held = rooms
        .room("t1", &mut link, || panic!("a held room is not read"))
        .unwrap();
    assert_eq!(held.version, Some(3));
    rooms.room("t2", &mut link, || Ok(room("t2", 5))).unwrap();
    let receipt = rooms.receipt();
    assert_eq!(
        (
            receipt["heads"].as_str(),
            receipt["reused"].as_u64(),
            receipt["rooms_fetched"].as_u64()
        ),
        (Some("held"), Some(1), Some(1))
    );
}

#[test]
fn extra_print_inputs_are_kept_whole_beside_the_rooms() {
    let dir = tempfile::tempdir().unwrap();
    let hold = Hold::at(dir.path().to_path_buf(), "uid-1", "project-1");
    assert_eq!(hold.read_file("extra.json"), None);
    hold.keep_file("extra.json", b"{\"a\":1}").unwrap();
    hold.keep_file("extra.json", b"{\"a\":2}").unwrap();
    assert_eq!(
        hold.read_file("extra.json").as_deref(),
        Some(&b"{\"a\":2}"[..])
    );
    let other = Hold::at(dir.path().to_path_buf(), "uid-2", "project-1");
    assert_eq!(
        other.read_file("extra.json"),
        None,
        "another account holds its own"
    );
}
