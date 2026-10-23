use super::*;
use crate::session::{
    FeatureId, HoverQuery, PolygonSelectionQuery, SelectionMode, SessionHandle, SessionLayerInput,
    SnapQuery,
};
use serde::Serialize;
use serde_json::{Value, json};

const SCOPE: &str = "reply";
const POLES: &str = "reply:lv_poles";
const LINES: &str = "reply:lv_lines";

fn mounted() -> SessionHandle {
    let mut handle = SessionHandle::open(Vec::new());
    let layers: Vec<SessionLayerInput> = serde_json::from_value(json!([
        { "name": "lv_poles", "geojson": { "type": "FeatureCollection", "features": [
            { "type": "Feature", "id": "p1", "geometry": { "type": "Point", "coordinates": [30.06, -1.95] },
              "properties": { "id": "p1", "label": "A" } },
            { "type": "Feature", "id": "p2", "geometry": { "type": "Point", "coordinates": [30.0605, -1.9505] },
              "properties": { "id": "p2", "label": "B" } }
        ] } },
        { "name": "lv_lines", "geojson": { "type": "FeatureCollection", "features": [
            { "type": "Feature", "id": "l1", "geometry": { "type": "LineString",
              "coordinates": [[30.06, -1.95], [30.0605, -1.9505], [30.061, -1.951]] },
              "properties": { "id": "l1" } }
        ] } }
    ]))
    .unwrap();
    assert_eq!(handle.mount_scope_layers(SCOPE, layers).unwrap(), 3);
    handle
}

fn query<T: serde::de::DeserializeOwned>(value: Value) -> T {
    serde_json::from_value(value).unwrap()
}

fn text<T: Serialize>(value: &T) -> String {
    serde_json::to_string(value).unwrap()
}

/// Every reply family, as the Kernel adapter writes it from a real session.
fn kernel_replies() -> Vec<(ReplyFamily, String)> {
    let mut handle = mounted();
    let mut replies = vec![
        (
            ReplyFamily::ScopeStatus,
            text(&handle.scope_status(SCOPE).unwrap()),
        ),
        (ReplyFamily::Projection, text(&handle.projection_metadata())),
    ];
    let vertex = handle.snap(query::<SnapQuery>(json!({
        "lng": 30.06, "lat": -1.95, "toleranceM": 5, "targetLayers": [POLES], "snapToVertices": true
    })));
    let edge = handle.snap(query::<SnapQuery>(json!({
        "lng": 30.06026, "lat": -1.95024, "toleranceM": 5, "targetLayers": [LINES], "snapToSegments": true
    })));
    let own = handle.snap(query::<SnapQuery>(json!({
        "lng": 31.0, "lat": -1.0, "toleranceM": 1, "snapToSelf": true, "selfVertices": [[31.0, -1.0]]
    })));
    for snap in [vertex, edge, own] {
        replies.push((ReplyFamily::Snap, text(&snap.expect("snap hit"))));
    }
    let feature = handle.hover_at(query::<HoverQuery>(json!({
        "lng": 30.06, "lat": -1.95, "toleranceM": 5, "targetLayers": [POLES]
    })));
    let segment = handle.hover_at(query::<HoverQuery>(json!({
        "lng": 30.06026, "lat": -1.95024, "toleranceM": 5, "targetLayers": [LINES]
    })));
    for hover in [feature, segment] {
        replies.push((ReplyFamily::Hover, text(&hover.expect("hover hit"))));
    }
    replies.push((ReplyFamily::Selection, text(handle.selection())));
    handle.select(
        vec![FeatureId::new(format!("{POLES}/p1"))],
        SelectionMode::Replace,
    );
    handle.hover(Some(FeatureId::new(format!("{POLES}/p2"))));
    let marked = handle.mark_edited(FeatureId::new(format!("{LINES}/l1")));
    replies.push((ReplyFamily::Selection, text(&marked)));
    let lassoed = handle
        .select_by_polygon(query::<PolygonSelectionQuery>(json!({
            "polygon": { "type": "Polygon", "coordinates": [[
                [30.0599, -1.9501], [30.0601, -1.9501], [30.0601, -1.9499], [30.0599, -1.9499], [30.0599, -1.9501]
            ]] },
            "targetLayers": [POLES], "mode": "replace"
        })))
        .unwrap();
    replies.push((ReplyFamily::Selection, text(&lassoed)));

    let created = handle
        .create_geojson_feature(
            POLES,
            json!({ "type": "Point", "coordinates": [30.07, -1.96] }),
            json!({ "label": "C" }).as_object().cloned().unwrap(),
        )
        .unwrap()
        .expect("created");
    let moved = handle
        .move_vertex(
            &FeatureId::new(format!("{LINES}/l1")),
            0,
            1,
            [30.0606, -1.9506],
        )
        .unwrap()
        .expect("moved");
    let undone = handle.undo().unwrap().expect("undone");
    for diff in [&created, &moved, &undone] {
        replies.push((ReplyFamily::Diff, text(diff)));
    }
    #[cfg(feature = "enable_dataframes")]
    {
        let row = handle.get_row(POLES, "p1").unwrap();
        assert!(row.is_some());
        replies.push((ReplyFamily::Row, text(&row)));
        let edit = handle.edit_cell(POLES, "p1", "label", json!("Z")).unwrap();
        assert!(edit.is_some());
        replies.push((ReplyFamily::CellEdit, text(&edit)));
        let fresh_column = handle
            .edit_cell(POLES, "p2", "note", json!({ "kind": "fresh" }))
            .unwrap();
        assert_eq!(
            fresh_column.as_ref().and_then(|diff| diff.before.as_ref()),
            None
        );
        replies.push((ReplyFamily::CellEdit, text(&fresh_column)));
        let batch = handle
            .edit_cells(
                POLES,
                &["p1".to_string(), "absent".to_string()],
                json!({ "label": "Y" }).as_object().cloned().unwrap(),
            )
            .unwrap();
        replies.push((ReplyFamily::CellEdits, text(&batch)));
    }
    replies
}

/// Decode with the family's owner and write the admitted value back as JSON.
fn admit(family: ReplyFamily, reply: &str) -> Result<Value, ReplyError> {
    fn value<T: Serialize>(decoded: Result<T, ReplyError>) -> Result<Value, ReplyError> {
        decoded.map(|decoded| serde_json::to_value(decoded).unwrap())
    }
    match family {
        ReplyFamily::Diff => value(decode_diff(reply)),
        ReplyFamily::ScopeStatus => value(decode_scope_status(reply)),
        ReplyFamily::Projection => value(decode_projection(reply)),
        ReplyFamily::Snap => value(decode_snap(reply)),
        ReplyFamily::Hover => value(decode_hover(reply)),
        ReplyFamily::Selection => value(decode_selection(reply)),
        #[cfg(feature = "enable_dataframes")]
        ReplyFamily::Row => value(decode_row(reply)),
        #[cfg(feature = "enable_dataframes")]
        ReplyFamily::CellEdit => value(decode_cell_edit(reply)),
        #[cfg(feature = "enable_dataframes")]
        ReplyFamily::CellEdits => value(decode_cell_edits(reply)),
        #[cfg(not(feature = "enable_dataframes"))]
        ReplyFamily::Row | ReplyFamily::CellEdit | ReplyFamily::CellEdits => {
            unreachable!("table replies need enable_dataframes")
        }
    }
}

/// Fields the owning type declares optional. Every other written field is
/// required: `properties` is the Feature contract's own empty default, and
/// the rest are written only when they apply.
fn optional_fields(family: ReplyFamily) -> &'static [&'static str] {
    match family {
        ReplyFamily::Diff => &["before", "after", "properties"],
        ReplyFamily::ScopeStatus | ReplyFamily::Projection => &["utm_zone", "utm_southern"],
        ReplyFamily::Snap => &["ringIndex", "vertexIndex", "segmentIndex", "t"],
        ReplyFamily::Hover => &["coord", "ringIndex", "vertexIndex", "segmentIndex", "t"],
        ReplyFamily::Selection
        | ReplyFamily::Row
        | ReplyFamily::CellEdit
        | ReplyFamily::CellEdits => &[],
    }
}

/// Members whose value is free-form JSON: feature properties and cell values.
fn free_form(family: ReplyFamily, key: &str) -> bool {
    key == "properties"
        || matches!(family, ReplyFamily::CellEdit | ReplyFamily::CellEdits)
            && matches!(key, "before" | "after")
}

/// JSON pointers of every typed object member.
fn member_pointers(family: ReplyFamily, value: &Value, at: &str, out: &mut Vec<String>) {
    match value {
        Value::Object(map) => {
            for (key, member) in map {
                let pointer = format!("{at}/{key}");
                out.push(pointer.clone());
                if !free_form(family, key) {
                    member_pointers(family, member, &pointer, out);
                }
            }
        }
        Value::Array(items) => {
            for (index, item) in items.iter().enumerate() {
                member_pointers(family, item, &format!("{at}/{index}"), out);
            }
        }
        _ => {}
    }
}

fn without(root: &Value, pointer: &str) -> (String, Value) {
    let (parent, key) = pointer.rsplit_once('/').unwrap();
    let mut reduced = root.clone();
    reduced
        .pointer_mut(parent)
        .and_then(Value::as_object_mut)
        .unwrap()
        .remove(key)
        .unwrap();
    (key.to_string(), reduced)
}

/// Add `key` to every typed object of the reply.
fn with_member_everywhere(family: ReplyFamily, value: &mut Value, key: &str, member: &Value) {
    match value {
        Value::Object(map) => {
            for (name, nested) in map.iter_mut() {
                if !free_form(family, name) {
                    with_member_everywhere(family, nested, key, member);
                }
            }
            map.insert(key.to_string(), member.clone());
        }
        Value::Array(items) => {
            for item in items {
                with_member_everywhere(family, item, key, member);
            }
        }
        _ => {}
    }
}

#[test]
fn every_reply_family_round_trips_through_its_owner() {
    let replies = kernel_replies();
    let mut families: Vec<_> = replies.iter().map(|(family, _)| family.as_str()).collect();
    families.dedup();
    let expected: &[&str] = if cfg!(feature = "enable_dataframes") {
        &[
            "scope_status",
            "projection",
            "snap",
            "hover",
            "selection",
            "diff",
            "row",
            "cell_edit",
            "cell_edits",
        ]
    } else {
        &[
            "scope_status",
            "projection",
            "snap",
            "hover",
            "selection",
            "diff",
        ]
    };
    assert_eq!(families, expected);
    for (family, reply) in &replies {
        let written: Value = serde_json::from_str(reply).unwrap();
        assert_eq!(
            admit(*family, reply).unwrap(),
            written,
            "{} reply changed through its owner: {reply}",
            family.as_str()
        );
    }
}

#[test]
fn typed_replies_equal_the_kernel_values() {
    let mut handle = mounted();
    let status = handle.scope_status(SCOPE).unwrap();
    assert_eq!(decode_scope_status(&text(&status)).unwrap(), status);
    let projection = handle.projection_metadata();
    assert_eq!(decode_projection(&text(&projection)).unwrap(), projection);
    let selection = handle.select(
        vec![FeatureId::new(format!("{POLES}/p2"))],
        SelectionMode::Add,
    );
    assert_eq!(decode_selection(&text(&selection)).unwrap(), selection);
    let diff = handle
        .remove_diff(&FeatureId::new(format!("{POLES}/p2")))
        .unwrap()
        .expect("removed");
    assert!(diff.changes[0].after.is_none());
    assert_eq!(decode_diff(&text(&diff)).unwrap(), diff);
    #[cfg(feature = "enable_dataframes")]
    {
        let row = handle.get_row(POLES, "p1").unwrap();
        assert_eq!(decode_row(&text(&row)).unwrap(), row);
        assert_eq!(decode_row("null").unwrap(), None);
        let absent = handle
            .edit_cell(POLES, "absent", "label", json!("Q"))
            .unwrap();
        assert_eq!(absent, None);
        assert_eq!(decode_cell_edit(&text(&absent)).unwrap(), None);
        let batch = handle
            .edit_cells(
                POLES,
                &["p1".to_string()],
                json!({ "label": "Y" }).as_object().cloned().unwrap(),
            )
            .unwrap();
        assert_eq!(decode_cell_edits(&text(&batch)).unwrap(), batch);
        // A cell set to null and an absent cell are one wire state (`null`);
        // the decoded reply writes back exactly what the Kernel wrote.
        let cleared = handle
            .edit_cells(
                POLES,
                &["p1".to_string()],
                json!({ "label": null }).as_object().cloned().unwrap(),
            )
            .unwrap();
        assert_eq!(cleared.changed[0].after, Some(Value::Null));
        let decoded = decode_cell_edits(&text(&cleared)).unwrap();
        assert_eq!(decoded.changed[0].after, None);
        assert_eq!(text(&decoded), text(&cleared));
    }
}

#[test]
fn a_reply_missing_a_written_field_is_refused() {
    let mut checked = 0;
    for (family, reply) in kernel_replies() {
        let written: Value = serde_json::from_str(&reply).unwrap();
        let mut pointers = Vec::new();
        member_pointers(family, &written, "", &mut pointers);
        assert!(!pointers.is_empty() || written.is_null());
        for pointer in pointers {
            let (key, reduced) = without(&written, &pointer);
            let outcome = admit(family, &reduced.to_string());
            if optional_fields(family).contains(&key.as_str()) {
                assert!(
                    outcome.is_ok(),
                    "{} reply refused without optional {pointer}: {outcome:?}",
                    family.as_str()
                );
            } else {
                let error = outcome.expect_err(&format!(
                    "{} reply admitted without written {pointer}",
                    family.as_str()
                ));
                assert_eq!(error.family, family);
                checked += 1;
            }
        }
    }
    assert!(
        checked > 60,
        "only {checked} required fields were exercised"
    );
}

#[test]
fn a_reply_written_as_null_must_stay_present() {
    let cleared = r#"{"selected":[],"hovered":null,"last_edited":null}"#;
    assert_eq!(
        decode_selection(cleared).unwrap(),
        SelectionState::default()
    );
    for reply in [
        r#"{"selected":[],"last_edited":null}"#,
        r#"{"selected":[],"hovered":null}"#,
    ] {
        let error = decode_selection(reply).unwrap_err();
        assert!(error.message.contains("missing field"), "{error}");
    }
    let own = r#"{"targetKind":"self","featureId":null,"layerId":null,"coord":[31.0,-1.0],"distanceM":0.0}"#;
    assert_eq!(decode_snap(own).unwrap().feature_id, None);
    let unlayered = own.replace(r#""layerId":null,"#, "");
    assert!(
        decode_snap(&unlayered)
            .unwrap_err()
            .message
            .contains("layerId")
    );
}

#[test]
fn a_reply_of_the_wrong_type_is_refused() {
    let replies = kernel_replies();
    let reply = |family: ReplyFamily| -> Value {
        let (_, text) = replies.iter().find(|(owner, _)| *owner == family).unwrap();
        serde_json::from_str(text).unwrap()
    };
    let mutated = |family: ReplyFamily, pointer: &str, replacement: Value| {
        let mut value = reply(family);
        *value
            .pointer_mut(pointer)
            .unwrap_or_else(|| panic!("{pointer}")) = replacement;
        (family, value.to_string())
    };
    let mut cases = vec![
        mutated(ReplyFamily::ScopeStatus, "/featureCount", json!("2")),
        mutated(ReplyFamily::ScopeStatus, "/inputFeatureCount", json!(-1)),
        mutated(ReplyFamily::ScopeStatus, "/totalFeatureCount", json!(1.5)),
        mutated(ReplyFamily::ScopeStatus, "/mounted", json!("true")),
        mutated(ReplyFamily::ScopeStatus, "/layers", json!({})),
        mutated(
            ReplyFamily::ScopeStatus,
            "/projection/mode",
            json!("mercator"),
        ),
        mutated(ReplyFamily::Projection, "/origin_lng", json!(null)),
        mutated(ReplyFamily::Snap, "/targetKind", json!("corner")),
        mutated(ReplyFamily::Snap, "/coord", json!([30.06])),
        mutated(ReplyFamily::Hover, "/featureId", json!(null)),
        mutated(ReplyFamily::Hover, "/targetKind", json!("edge")),
        mutated(
            ReplyFamily::Selection,
            "/selected",
            json!("reply:lv_poles/p1"),
        ),
        mutated(ReplyFamily::Selection, "/hovered", json!(7)),
        mutated(ReplyFamily::Diff, "/changes", json!({})),
        mutated(
            ReplyFamily::Diff,
            "/changes/0/after/geometry/type",
            json!("MultiPoint"),
        ),
        mutated(
            ReplyFamily::Diff,
            "/changes/0/after/geometry/coordinates",
            json!([[30.07, -1.96]]),
        ),
        mutated(ReplyFamily::Diff, "/changes/0/after/properties", json!([])),
    ];
    if cfg!(feature = "enable_dataframes") {
        cases.extend([
            mutated(ReplyFamily::Row, "/row_index", json!("0")),
            mutated(ReplyFamily::Row, "/properties", json!("label=A")),
            mutated(ReplyFamily::CellEdit, "/feature_id", json!(1)),
            mutated(ReplyFamily::CellEdits, "/missing_feature_ids", json!([1])),
            mutated(ReplyFamily::CellEdits, "/changed", json!(null)),
        ]);
    }
    for (family, reply) in cases {
        let error =
            admit(family, &reply).expect_err(&format!("{} admitted {reply}", family.as_str()));
        assert_eq!(error.family, family);
    }
}

#[test]
fn a_reply_that_is_not_one_json_object_is_refused() {
    for (family, reply) in kernel_replies() {
        let written: Value = serde_json::from_str(&reply).unwrap();
        let positional = Value::Array(
            written
                .as_object()
                .map(|map| map.values().cloned().collect())
                .unwrap_or_default(),
        );
        for malformed in [
            String::new(),
            "not json".to_string(),
            "[]".to_string(),
            positional.to_string(),
            "\"reply\"".to_string(),
            "42".to_string(),
            format!("{reply} trailing"),
            reply[..reply.len() - 1].to_string(),
        ] {
            let error = admit(family, &malformed)
                .expect_err(&format!("{} admitted {malformed:?}", family.as_str()));
            assert_eq!(error.family, family);
            assert!(
                error.to_string().starts_with(&format!(
                    "design_session_response_invalid: {} reply ",
                    family.as_str()
                )),
                "{error}"
            );
        }
        let absent = admit(family, "null");
        match family {
            ReplyFamily::Row | ReplyFamily::CellEdit => assert_eq!(absent.unwrap(), Value::Null),
            _ => assert!(absent.is_err(), "{} admitted null", family.as_str()),
        }
    }
}

#[test]
fn additive_reply_fields_stay_compatible() {
    let member = json!({ "addedBy": "a newer Kernel", "items": [1, 2] });
    for (family, reply) in kernel_replies() {
        let written: Value = serde_json::from_str(&reply).unwrap();
        let mut extended = written.clone();
        with_member_everywhere(family, &mut extended, "kernelFutureField", &member);
        assert_ne!(extended, written);
        assert_eq!(
            admit(family, &extended.to_string()).unwrap(),
            written,
            "{} reply refused or kept an additive field",
            family.as_str()
        );
    }
}

/// Every object of a reply, its keys written in reverse order.
fn reversed_keys(value: &Value) -> Value {
    match value {
        Value::Object(map) => Value::Object(
            map.iter()
                .rev()
                .map(|(key, member)| (key.clone(), reversed_keys(member)))
                .collect(),
        ),
        Value::Array(items) => Value::Array(items.iter().map(reversed_keys).collect()),
        other => other.clone(),
    }
}

#[test]
fn reply_key_order_is_not_significant() {
    for (family, reply) in kernel_replies() {
        let written: Value = serde_json::from_str(&reply).unwrap();
        let reordered = reversed_keys(&written).to_string();
        // Key order is kept in this build, so the fixture is really unsorted.
        assert_ne!(
            reordered,
            written.to_string(),
            "{} fixture",
            family.as_str()
        );
        assert_eq!(
            admit(family, &reordered).unwrap(),
            written,
            "{} reply depends on key order",
            family.as_str()
        );
    }
}
