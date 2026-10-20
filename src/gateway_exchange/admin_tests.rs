use super::*;
use crate::gateway_exchange::{resume, start, Facts, Method, Observed, Step};
use serde_json::json;

fn answered<C: Exchange>(call: C, status: u16, body: Value) -> Result<C::Output, Refusal> {
    let Step::Fetch { continuation, .. } = start(call) else {
        panic!("expected a fetch")
    };
    let observed = Observed::Response {
        status,
        content_type: Some("application/json".into()),
        retry_after: None,
    };
    match resume(
        continuation,
        observed,
        body.to_string().as_bytes(),
        &Facts::default(),
    ) {
        Step::Done(value) => Ok(value),
        Step::Refused(refusal) => Err(refusal),
        _ => panic!("expected an outcome"),
    }
}

fn request<C: Exchange>(call: C) -> Request {
    match start(call) {
        Step::Fetch { request, .. } => request,
        Step::Refused(refusal) => panic!("refused: {refusal:?}"),
        _ => panic!("expected a fetch"),
    }
}

fn pairs(request: &Request) -> Vec<(&str, &str)> {
    request
        .query
        .iter()
        .map(|(key, value)| (key.as_str(), value.as_str()))
        .collect()
}

#[test]
fn children_are_a_get_by_level_and_parent() {
    let call = Children {
        level: "district".into(),
        parent_code: Some("1".into()),
        timeout_ms: None,
    };
    let sent = request(call.clone());
    assert_eq!(
        (sent.method, sent.path.as_str()),
        (Method::Get, "/api/v1/admin/rwanda")
    );
    assert_eq!(
        pairs(&sent),
        [
            ("action", "children"),
            ("level", "district"),
            ("parent_code", "1")
        ]
    );
    assert_eq!(sent.timeout_ms, 30_000);
    let rows = answered(
        call,
        200,
        json!({"level": "district", "parent_code": "1",
        "children": [{"name": "Nyarugenge", "code": "11"}]}),
    )
    .unwrap();
    assert_eq!(
        rows,
        [AdminChild {
            code: "11".into(),
            name: "Nyarugenge".into()
        }]
    );
    let provinces = Children {
        level: "province".into(),
        parent_code: None,
        timeout_ms: Some(120_000),
    };
    let sent = request(provinces);
    assert_eq!(
        pairs(&sent),
        [("action", "children"), ("level", "province")]
    );
    assert_eq!(sent.timeout_ms, 120_000);
}

#[test]
fn a_children_answer_without_rows_is_refused() {
    let call = Children {
        level: "province".into(),
        parent_code: None,
        timeout_ms: None,
    };
    assert_eq!(
        answered(call, 200, json!({"level": "province"}))
            .unwrap_err()
            .code,
        INVALID
    );
}

#[test]
fn an_unknown_level_code_or_deadline_is_never_sent() {
    let refused =
        |step: Step<Children>| matches!(step, Step::Refused(r) if r.code == INPUT_INVALID);
    assert!(refused(start(Children {
        level: "country".into(),
        parent_code: None,
        timeout_ms: None
    })));
    assert!(refused(start(Children {
        level: "cell".into(),
        parent_code: Some("1;drop".into()),
        timeout_ms: None
    })));
    assert!(refused(start(Children {
        level: "cell".into(),
        parent_code: None,
        timeout_ms: Some(10)
    })));
    assert!(
        matches!(start(Boundary { code: String::new(), timeout_ms: None }),
        Step::Refused(r) if r.code == INPUT_INVALID)
    );
    assert!(
        matches!(start(Locate { lat: 91.0, lon: 30.0, level: "village".into() }),
        Step::Refused(r) if r.code == INPUT_INVALID)
    );
}

#[test]
fn a_boundary_carries_its_geometry_or_none() {
    let shape = json!({"type": "Polygon", "coordinates": [[[30.0, -2.0], [30.1, -2.0], [30.0, -2.1], [30.0, -2.0]]]});
    let sent = request(Boundary {
        code: "1102".into(),
        timeout_ms: None,
    });
    assert_eq!(pairs(&sent), [("action", "geometry"), ("code", "1102")]);
    let boundary = answered(
        Boundary {
            code: "1102".into(),
            timeout_ms: None,
        },
        200,
        json!({"level": "sector", "name": "Gitega", "code": "1102", "geometry": shape}),
    )
    .unwrap();
    assert_eq!(boundary.level, "sector");
    assert_eq!(boundary.geometry.unwrap()["type"], "Polygon");
    let empty = answered(
        Boundary {
            code: "1102".into(),
            timeout_ms: None,
        },
        200,
        json!({"level": "sector", "name": "Gitega", "code": "1102", "geometry": null}),
    )
    .unwrap();
    assert_eq!(empty.geometry, None);
}

#[test]
fn descendants_keep_each_villages_hierarchy() {
    let call = Descendants {
        level: "village".into(),
        parent_code: "11".into(),
        timeout_ms: None,
    };
    let sent = request(call.clone());
    assert_eq!(
        pairs(&sent),
        [
            ("action", "descendants"),
            ("level", "village"),
            ("parent_code", "11")
        ]
    );
    let rows = answered(
        call,
        200,
        json!({"level": "village", "parent_code": "11", "count": 1,
        "boundaries": [{"name": "Kiyovu", "code": "11020301", "geometry": null,
            "cell": "Kiyovu", "sector": "Nyarugenge", "district": "Nyarugenge"}]}),
    )
    .unwrap();
    assert_eq!(rows[0].level, "village");
    assert_eq!(rows[0].district.as_deref(), Some("Nyarugenge"));
}

#[test]
fn a_name_and_a_location_are_read() {
    let name = answered(
        Name { code: "11".into() },
        200,
        json!({"level": "district", "code": "11", "name": "Nyarugenge"}),
    )
    .unwrap();
    assert_eq!(name.name, "Nyarugenge");
    let call = Locate {
        lat: -1.95,
        lon: 30.06,
        level: "village".into(),
    };
    let sent = request(call.clone());
    assert_eq!(pairs(&sent)[0], ("action", "locate"));
    let found = answered(
        call.clone(),
        200,
        json!({"level": "village", "lat": -1.95, "lon": 30.06,
        "found": true, "name": "Kiyovu", "code": "11020301", "district": "Nyarugenge",
        "district_code": "11", "province": "Kigali", "province_code": "1"}),
    )
    .unwrap();
    assert!(found.found);
    assert_eq!(found.province_code.as_deref(), Some("1"));
    let outside = answered(
        call,
        200,
        json!({"level": "village", "lat": 0, "lon": 0, "found": false}),
    )
    .unwrap();
    assert!(!outside.found && outside.name.is_none());
}

#[cfg(feature = "durable-store")]
mod held_children {
    use super::super::{held_children, refresh_children, AdminChild, Children};
    use crate::gateway_exchange::retained::testing::{block_on, TestEdge};
    use serde_json::json;

    fn districts() -> Children {
        Children {
            level: "district".into(),
            parent_code: Some("1".into()),
            timeout_ms: None,
        }
    }

    #[test]
    fn refreshed_children_open_offline_and_never_for_another_account() {
        let edge = TestEdge::new();
        assert_eq!(
            block_on(held_children(&edge, "district", Some("1"))).unwrap(),
            None
        );
        edge.json(json!({"children": [{"code": "11", "name": "Nyarugenge"}]}));
        let fresh = block_on(refresh_children(&edge, districts())).unwrap();
        assert_eq!(
            fresh,
            [AdminChild {
                code: "11".into(),
                name: "Nyarugenge".into()
            }]
        );
        edge.online.set(false);
        assert!(block_on(refresh_children(&edge, districts())).is_err());
        assert_eq!(
            block_on(held_children(&edge, "district", Some("1"))).unwrap(),
            Some(fresh)
        );
        assert_eq!(
            block_on(held_children(&edge, "district", Some("2"))).unwrap(),
            None
        );
        edge.sign_in("someone-else");
        assert_eq!(
            block_on(held_children(&edge, "district", Some("1"))).unwrap(),
            None
        );
    }
}

#[cfg(feature = "durable-store")]
#[test]
fn a_boundary_is_held_offline_and_when_no_answer_arrives_but_never_over_a_refusal() {
    use crate::gateway_exchange::retained::testing::{block_on, Answer, TestEdge};
    use crate::gateway_exchange::Failure;
    let edge = TestEdge::new();
    let call = || Boundary {
        code: "1102".into(),
        timeout_ms: None,
    };
    let unit = json!({"level": "sector", "name": "Gitega", "code": "1102", "geometry": null});
    edge.json(json!({"success": true, "data": unit}));
    let fresh = block_on(boundary(&edge, call())).unwrap();
    edge.online.set(false);
    assert_eq!(block_on(boundary(&edge, call())).unwrap(), fresh);
    edge.online.set(true);
    edge.answer(Answer::Failed(Failure::Network));
    assert_eq!(block_on(boundary(&edge, call())).unwrap(), fresh);
    edge.answer(Answer::Json(403, json!({"success": false, "error": {"code": "FORBIDDEN", "message": "no"}})));
    assert!(block_on(boundary(&edge, call())).is_err());
    let other = Boundary {
        code: "1103".into(),
        timeout_ms: None,
    };
    edge.online.set(false);
    assert!(block_on(boundary(&edge, other)).is_err(), "nothing held and no link");
}
