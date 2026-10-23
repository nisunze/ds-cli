use super::*;

fn submit(component: &str) -> Command {
    Command::Submit {
        title: "t".into(),
        detail: "d".into(),
        component: component.into(),
        kind: "bug".into(),
        severity: "minor".into(),
        agent: "a".into(),
        model: None,
        client: None,
        evidence: Vec::new(),
        context: BTreeMap::new(),
        replay: None,
    }
}

/// FEEDBACK `9ab57db2`: a report names a live repository, optionally with
/// an area; a retired or invented label is refused, and a legacy label's
/// refusal can name the repository that owns it.
#[test]
fn a_report_names_a_live_repository_or_is_refused() {
    for live in [
        "ds-cli",
        "ds-cli/feedback",
        "ds-network/spotting",
        "simple_notes",
    ] {
        assert!(submit(live).validate().is_ok(), "{live} is accepted");
    }
    for unknown in [
        "ds-vector-tiler",
        "ds-vector-tiler/tiles",
        "ds-solar-weather",
        "ds-solar-weather/weather",
        "ds-grid",
        "ds-grid-engine/sag",
        "platform",
        "ds",
        "ds-desktop/x",
    ] {
        let error = submit(unknown).validate().unwrap_err();
        assert_eq!(error.to_string(), COMPONENT_UNKNOWN, "{unknown}");
    }
    assert_eq!(legacy_owner("ds-grid-engine"), Some("ds-network"));
    assert_eq!(legacy_owner("ds-report-pdf"), Some("ds-network-reporter"));
    assert_eq!(legacy_owner("ds pls desktop (windows)"), Some("ds-cli"));
    assert_eq!(legacy_owner("platform"), None);
    // A component the bounds already refuse keeps its bounded refusal.
    assert_ne!(
        submit(" ds-cli").validate().unwrap_err().to_string(),
        COMPONENT_UNKNOWN
    );
}

fn list(cursor: String) -> Command {
    Command::List {
        view: "not_addressed".into(),
        component: Some("ds-cli/feedback".into()),
        query: None,
        limit: 1,
        detail: false,
        cursor: Some(cursor),
        all: false,
    }
}

#[test]
fn bounded_page_tokens_accept_admitted_long_ids_without_local_decoding() {
    for size in [741, Command::MAX_CURSOR_BYTES] {
        assert!(list(format!("fb1.{}", "a".repeat(size - 4)))
            .validate()
            .is_ok());
    }
    assert!(list("fb1.legacy.digest".into()).validate().is_ok());
}

#[test]
fn oversized_and_non_token_cursors_are_refused_before_transport() {
    for cursor in [
        format!("fb1.{}", "a".repeat(Command::MAX_CURSOR_BYTES - 3)),
        format!("fb1.{}", "é".repeat(Command::MAX_CURSOR_BYTES / 2)),
        "forged-without-prefix".into(),
        "".into(),
    ] {
        assert_eq!(
            list(cursor).validate().unwrap_err().kind(),
            ErrorKind::InvalidInput
        );
    }
}

fn replay() -> Replay {
    Replay {
        version: "0.1.0".into(),
        source_sha: "eaf7eb823141ebb39c24e6b91c444e6cfd73cec0".into(),
        dirty: false,
        channel: "stable".into(),
        os: "linux".into(),
        arch: "x86_64".into(),
        command: "feedback.list".into(),
        argv: ["feedback", "list", "--limit", "50", "--output", "json"]
            .map(String::from)
            .to_vec(),
        redacted: Vec::new(),
        elided: Vec::new(),
        panic_location: Some("crates/cli/ds-cli-feedback/src/list.rs:120".into()),
    }
}

fn with_replay(replay: Option<Replay>, context: &[(&str, &str)]) -> Command {
    let Command::Submit {
        title,
        detail,
        component,
        kind,
        severity,
        agent,
        ..
    } = submit("ds-cli/feedback")
    else {
        unreachable!()
    };
    Command::Submit {
        title,
        detail,
        component,
        kind,
        severity,
        agent,
        model: None,
        client: None,
        evidence: Vec::new(),
        context: context
            .iter()
            .map(|(key, value)| (key.to_string(), value.to_string()))
            .collect(),
        replay,
    }
}

/// A replay is one context entry on the wire, so a deployed backlog keeps it
/// per sighting; it decodes back from a stored report's context, and a
/// report without one, or with a malformed one, simply has none.
#[test]
fn a_replay_travels_as_one_context_entry_and_decodes_back() {
    let replay = replay();
    assert!(replay.is_valid());
    let context = BTreeMap::from([(REPLAY_CONTEXT_KEY.to_string(), replay.encode())]);
    assert_eq!(Replay::from_context(&context), Some(replay.clone()));
    assert_eq!(Replay::from_context(&BTreeMap::new()), None);
    let forged = BTreeMap::from([(REPLAY_CONTEXT_KEY.to_string(), "{\"argv\":[]}".into())]);
    assert_eq!(Replay::from_context(&forged), None);

    let mut report: Report = serde_json::from_value(json!({
        "id":"f1","status":"open","kind":"bug","severity":"major","component":"ds-cli",
        "surface":"cli","title":"t","detail":"d","occurrences":1,"reporters":["a@b.c"],
        "reporter_kind":"agent","version":1,"last_seen_at":"2026-10-23T12:00:00Z",
        "updated_at":"2026-10-23T12:00:00Z","updated_by":"a@b.c",
        "context":{"replay": replay.encode(), "sre_event": "inv_1"},
    }))
    .unwrap();
    report.validate().unwrap();
    let full = report.row(true);
    assert_eq!(full["replay"], json!(replay));
    assert!(
        full.get("context").is_none(),
        "a row adds the replay, not the context"
    );
    report = serde_json::from_value(json!({
        "id":"f1","status":"open","kind":"bug","severity":"major","component":"ds-cli",
        "surface":"cli","title":"t","detail":"d","occurrences":1,"reporters":["a@b.c"],
        "reporter_kind":"agent","version":1,"last_seen_at":"2026-10-23T12:00:00Z",
        "updated_at":"2026-10-23T12:00:00Z","updated_by":"a@b.c",
        "context":{"replay": replay.encode()},
    }))
    .unwrap();
    assert!(
        report.row(false).get("replay").is_none(),
        "a bounded row stays bounded; --detail carries the replay"
    );
}

/// The replay key is reserved, counts against the 24 context entries, and an
/// invalid or over-long replay is refused before anything is sent.
#[test]
fn a_replay_is_bounded_and_its_key_is_reserved() {
    assert!(with_replay(Some(replay()), &[]).validate().is_ok());
    assert!(with_replay(None, &[("replay", "{}")]).validate().is_err());
    let full: Vec<(String, String)> = (0..24)
        .map(|i| (format!("k{i}"), "v".to_string()))
        .collect();
    let full: Vec<(&str, &str)> = full.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
    assert!(with_replay(None, &full).validate().is_ok());
    assert!(with_replay(Some(replay()), &full).validate().is_err());

    let mut long = replay();
    long.argv.push("x".repeat(REPLAY_MAX_CHARS));
    assert!(!long.is_valid());
    assert!(with_replay(Some(long), &[]).validate().is_err());
    for broken in [
        Replay {
            channel: "nightly".into(),
            ..replay()
        },
        Replay {
            source_sha: "not-a-sha".into(),
            ..replay()
        },
        Replay {
            command: "Feedback List".into(),
            ..replay()
        },
        Replay {
            panic_location: Some("no location".into()),
            ..replay()
        },
    ] {
        assert!(!broken.is_valid(), "{broken:?}");
    }
}
