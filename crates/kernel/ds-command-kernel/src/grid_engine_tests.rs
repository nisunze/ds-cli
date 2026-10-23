use super::*;
use ds_network_contracts::grid_unit::encode_reply;

fn text(value: &str) -> GridArg {
    GridArg::Text(value.to_string())
}

#[test]
fn effects_name_the_unit_entry_and_the_admitted_request() {
    let effect = request("session", "applyTransactionAtHead", vec![text("{}")]).unwrap();
    assert_eq!((effect.unit, effect.entry), ("network", GridEntry::Session));
    let decoded = GridRequest::decode(&effect.payload).unwrap();
    assert_eq!(
        decoded.operation,
        GridOperation::parse("session", "applyTransactionAtHead").unwrap()
    );
    assert_eq!(decoded.args, vec![text("{}")]);
    assert_eq!(
        request("open", "openDsgrid", vec![GridArg::Bytes(vec![1])])
            .unwrap()
            .entry,
        GridEntry::Session
    );
    assert_eq!(
        request("model", "emptyDsgridTemplate", vec![])
            .unwrap()
            .entry,
        GridEntry::Model
    );
}

#[test]
fn requests_outside_the_closed_set_or_shape_refuse_before_any_effect() {
    for (kind, method, args, code) in [
        (
            "session",
            "runSectionSpanLimitCheck",
            vec![text("{}")],
            METHOD_UNAVAILABLE,
        ),
        (
            "session",
            "applyCommand",
            vec![text("{}")],
            METHOD_UNAVAILABLE,
        ),
        ("model", "linkedModelLink", vec![], METHOD_UNAVAILABLE),
        ("call", "historyStateJson", vec![], REQUEST_INVALID),
        ("session", "applyTransactionAtHead", vec![], REQUEST_INVALID),
        (
            "session",
            "applyTransactionAtHead",
            vec![GridArg::Bytes(vec![])],
            REQUEST_INVALID,
        ),
        (
            "session",
            "resolveAlignmentStationJson",
            vec![text("a"), GridArg::Number(f64::INFINITY)],
            REQUEST_INVALID,
        ),
        (
            "open",
            "openDsgrid",
            vec![text("not bytes")],
            REQUEST_INVALID,
        ),
    ] {
        assert_eq!(
            request(kind, method, args).unwrap_err().code,
            code,
            "{kind}/{method}"
        );
    }
}

#[test]
fn replies_pass_through_verbatim_or_refuse_typed() {
    let session = GridOperation::parse("session", "currentRevisionJson").unwrap();
    // Key order and number spelling are the unit's, not re-serialized here.
    let body = "{\"z\":1,\"a\":1.50,\"revision_id\":\"rev:x\"}";
    let reply = encode_reply(session, &Ok(GridResult::Text(body.into())));
    assert_eq!(
        decode("session", "currentRevisionJson", &reply).unwrap(),
        GridResult::Text(body.into())
    );
    // The owner's refusal keeps its code, message and detail.
    let owner = GridRefusal {
        code: "stale_revision".into(),
        message: "moved on".into(),
        detail: Some("{}".into()),
    };
    let refused = encode_reply(session, &Err(owner.clone()));
    assert_eq!(
        decode("session", "currentRevisionJson", &refused).unwrap_err(),
        owner
    );
    // Crossed, truncated or foreign replies are the Kernel's response refusal.
    assert_eq!(
        decode("session", "historyStateJson", &reply)
            .unwrap_err()
            .code,
        RESPONSE_INVALID
    );
    assert_eq!(
        decode("session", "currentRevisionJson", &reply[..reply.len() - 2])
            .unwrap_err()
            .code,
        RESPONSE_INVALID
    );
    assert_eq!(
        decode("session", "currentRevisionJson", b"not a frame")
            .unwrap_err()
            .code,
        RESPONSE_INVALID
    );
    let open = GridOperation::parse("open", "openDsgrid").unwrap();
    assert_eq!(
        decode(
            "open",
            "openDsgrid",
            &encode_reply(open, &Ok(GridResult::Opened))
        )
        .unwrap(),
        GridResult::Opened
    );
    assert_eq!(
        decode(
            "open",
            "openDsgrid",
            &encode_reply(open, &Ok(GridResult::Text("{}".into())))
        )
        .unwrap_err()
        .code,
        RESPONSE_INVALID
    );
}

#[test]
fn an_absent_unit_is_its_own_typed_refusal() {
    let refusal = unavailable("no admitted unit in this realm");
    assert_eq!(refusal.code, ENGINE_UNAVAILABLE);
    let json: serde_json::Value = serde_json::from_str(&refusal.to_json()).unwrap();
    assert_eq!(json["code"], "grid_engine_unavailable");
}
