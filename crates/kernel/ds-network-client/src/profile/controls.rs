//! Edit receipt transport fences. Network owns commands and history transitions;
//! the client rejects crossed or incomplete acknowledgments.
use super::*;
use ds_network_contracts::profile_control::NativeProfileSessionControlOperationV1 as ProfileControlOperation;

pub(super) fn required_capability(
    control: &Value,
) -> Result<Option<NetworkCapability>, NetworkFailure> {
    let control: ProfileControlOperation =
        serde_json::from_value(control.clone()).map_err(|error| invalid(error.to_string()))?;
    Ok(if control.requires_station_move() {
        Some(NetworkCapability::ProfileStationMoveV1)
    } else if control.requires_retype() {
        Some(NetworkCapability::ProfileRetypeV1)
    } else {
        None
    })
}

pub(super) fn validate_reply(
    operation: &NativeProfileOperationV1,
    bytes: &[u8],
    model: &str,
    previous: &Value,
    history: &Value,
) -> Result<(), NetworkFailure> {
    if let NativeProfileOperationV1::QueryResults {
        scope,
        expected_engineering_input_root,
        query,
    } = operation
    {
        let value: Value =
            serde_json::from_slice(bytes).map_err(|error| invalid(error.to_string()))?;
        if previous != history
            || value["scope"] != *scope
            || value["model_id"] != model
            || value["history"] != *history
            || value["model_revision"] != history["model_revision"]
            || value["engineering_input_root"] != *expected_engineering_input_root
            || value["kind"]
                != serde_json::to_value(query.kind).map_err(|error| invalid(error.to_string()))?
            || value["source"]
                != serde_json::to_value(query.source).map_err(|error| invalid(error.to_string()))?
        {
            return Err(invalid(
                "Profile query result crossed its captured history, root, source or selector",
            ));
        }
        return Ok(());
    }
    if matches!(operation, NativeProfileOperationV1::ModelLibrary {}) {
        let projection: Value =
            serde_json::from_slice(bytes).map_err(|error| invalid(error.to_string()))?;
        return if previous == history && projection["model_revision"] == history["model_revision"] {
            Ok(())
        } else {
            Err(invalid(
                "Profile model-library read changed authored history or revision",
            ))
        };
    }
    let presentation = match operation {
        NativeProfileOperationV1::EditMode { .. }
        | NativeProfileOperationV1::SaveControl { .. } => false,
        NativeProfileOperationV1::Control { .. } | NativeProfileOperationV1::Discard { .. } => true,
        _ => return Ok(()),
    };
    let value: Value = serde_json::from_slice(bytes).map_err(|e| invalid(e.to_string()))?;
    if value["history"] != *history {
        return Err(invalid(
            "Profile edit receipt differs from acknowledged history",
        ));
    }
    if presentation
        && (value["presentation"]["model_id"] != model
            || value["presentation"]["history"] != *history
            || value["presentation"]["revision"]["revision_id"] != history["model_revision"]
            || !value["presentation"]["validation"].is_object()
            || !value["presentation"]["table_counts"].is_object())
    {
        return Err(invalid(
            "Profile edit receipt has no exact atomic owner presentation",
        ));
    }
    match operation {
        NativeProfileOperationV1::Control { operation } => {
            let control: ProfileControlOperation =
                serde_json::from_value(operation.clone()).map_err(|e| invalid(e.to_string()))?;
            if value["applied"] != control.is_mutating()
                || (!control.is_mutating() && history != previous)
            {
                return Err(invalid(
                    "Profile preview/control changed its admitted mutation contract",
                ));
            }
            if (control.requires_station_move() || control.requires_retype())
                && value["result"]["action"] != operation["action"]
            {
                return Err(invalid(
                    "Profile gesture receipt changed its declared action",
                ));
            }
        }
        NativeProfileOperationV1::EditMode { enabled } => {
            if value["enabled"] != *enabled || history != previous {
                return Err(invalid(
                    "Profile edit-mode admission changed history or requested mode",
                ));
            }
        }
        NativeProfileOperationV1::SaveControl { operation } => {
            let saving = value["saving"]
                .as_bool()
                .ok_or_else(|| invalid("Profile save exclusion has no state"))?;
            let id = value["save_id"].as_str();
            if history != previous
                || saving != id.is_some()
                || (!saving && !value["save_id"].is_null())
                || id.is_some_and(|s| {
                    s.is_empty()
                        || s.trim() != s
                        || s.len() > 200
                        || s.chars().any(char::is_control)
                })
            {
                return Err(invalid(
                    "Profile save exclusion changed its history or lease identity",
                ));
            }
            match operation["action"].as_str() {
                Some("status") => {}
                Some("begin") if saving && value["save_id"] == operation["save_id"] => {}
                Some("finish") if !saving => {}
                _ => {
                    return Err(invalid(
                        "Profile save exclusion differs from requested action",
                    ))
                }
            }
        }
        NativeProfileOperationV1::Discard { .. } => {
            if value["revision"]["revision_id"] != history["model_revision"] {
                return Err(invalid(
                    "Profile discard revision differs from acknowledged history",
                ));
            }
        }
        _ => unreachable!(),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn history() -> Value {
        json!({"model_revision":"rev:one","initial_revision":"rev:zero",
        "undo_depth":1,"redo_depth":0,"history_pin":"one"})
    }
    fn accept(op: NativeProfileOperationV1, value: Value, old: Value, next: Value) -> bool {
        validate_reply(
            &op,
            &serde_json::to_vec(&value).unwrap(),
            "one",
            &old,
            &next,
        )
        .is_ok()
    }
    #[test]
    fn edit_and_save_admissions_never_adopt_history_or_invent_lease_success() {
        let history = history();
        let edit = || NativeProfileOperationV1::EditMode { enabled: true };
        assert!(accept(
            edit(),
            json!({"history":history,"enabled":true}),
            history.clone(),
            history.clone()
        ));
        assert!(!accept(
            edit(),
            json!({"history":history,"enabled":false}),
            history.clone(),
            history.clone()
        ));
        let begin = || NativeProfileOperationV1::SaveControl {
            operation: json!({"action":"begin","save_id":"one"}),
        };
        for (saving, id, expected) in [
            (true, json!("one"), true),
            (false, Value::Null, false),
            (true, json!("two"), false),
            (true, Value::Null, false),
        ] {
            assert_eq!(
                accept(
                    begin(),
                    json!({"history":history,"saving":saving,"save_id":id}),
                    history.clone(),
                    history.clone()
                ),
                expected
            );
        }
    }
    #[test]
    fn edit_reply_requires_atomic_presentation_and_preview_cannot_move_history() {
        let history = history();
        let mut old = history.clone();
        old["undo_depth"] = json!(0);
        let mut value = json!({"history":history,"applied":true,"presentation":{"model_id":"one","history":history,
            "revision":{"revision_id":"rev:one"},"validation":{},"table_counts":{}}});
        let apply = || NativeProfileOperationV1::Control {
            operation: json!({"action":"undo"}),
        };
        assert!(accept(apply(), value.clone(), old.clone(), history.clone()));
        value["presentation"]["history"] = old.clone();
        assert!(!accept(
            apply(),
            value.clone(),
            old.clone(),
            history.clone()
        ));
        value["presentation"]["history"] = history.clone();
        value["applied"] = json!(false);
        let preview = || NativeProfileOperationV1::Control {
            operation: json!({"action":"preview_transaction","commands":[]}),
        };
        assert!(!accept(preview(), value.clone(), old, history.clone()));
        assert!(accept(preview(), value, history.clone(), history));
    }

    #[test]
    fn gesture_capabilities_are_closed_and_owner_specific() {
        let station = json!({"action":"preview_structure_station_move", "request":{
            "expected_revision":"rev:one", "structure_id":"one", "station_m":12.0},
            "options":{}, "held_axis_pin":"one"});
        assert_eq!(
            required_capability(&station).unwrap(),
            Some(NetworkCapability::ProfileStationMoveV1)
        );
        let retype = json!({"action":"apply_structure_retype", "request":{
            "expected_revision":"rev:one", "structure_ids":["one"], "structure_type_id":"new"}});
        assert_eq!(
            required_capability(&retype).unwrap(),
            Some(NetworkCapability::ProfileRetypeV1)
        );
        assert_eq!(
            required_capability(&json!({"action":"undo"})).unwrap(),
            None
        );
        let mut extra = station;
        extra["request"]["x_m"] = json!(42);
        assert!(required_capability(&extra).is_err());
        assert!(required_capability(&json!({"action":"arbitrary_method"})).is_err());
    }

    #[test]
    fn gesture_preview_keeps_exact_action_and_history() {
        let history = history();
        let operation = || NativeProfileOperationV1::Control {
            operation: json!({"action":"preview_structure_retype", "request":{
                "expected_revision":"rev:one", "structure_ids":["one"], "structure_type_id":"new"}}),
        };
        let mut value = json!({"history":history,"applied":false,
            "result":{"action":"preview_structure_retype","preview":{}},
            "presentation":{"model_id":"one","history":history,
                "revision":{"revision_id":"rev:one"},"validation":{},"table_counts":{}}});
        assert!(accept(
            operation(),
            value.clone(),
            history.clone(),
            history.clone()
        ));
        value["result"]["action"] = json!("preview_structure_station_move");
        assert!(!accept(
            operation(),
            value.clone(),
            history.clone(),
            history.clone()
        ));
        value["result"]["action"] = json!("preview_structure_retype");
        let mut previous = history.clone();
        previous["undo_depth"] = json!(0);
        assert!(!accept(operation(), value, previous, history));
    }

    #[test]
    fn model_library_read_is_exact_and_never_adopts_history() {
        let history = history();
        let op = || NativeProfileOperationV1::ModelLibrary {};
        let mut value = json!({"schema_version":1,"model_revision":"rev:one",
            "engineering_input_root":"root","areas":[]});
        assert!(accept(
            op(),
            value.clone(),
            history.clone(),
            history.clone()
        ));
        value["model_revision"] = json!("rev:other");
        assert!(!accept(
            op(),
            value.clone(),
            history.clone(),
            history.clone()
        ));
        value["model_revision"] = json!("rev:one");
        let mut previous = history.clone();
        previous["history_pin"] = json!("other");
        assert!(!accept(op(), value, previous, history));
    }

    #[test]
    fn engineering_query_receipt_cannot_cross_source_root_or_history() {
        let history = history();
        let operation = || NativeProfileOperationV1::QueryResults {
            scope: "profile-a".into(),
            expected_engineering_input_root: "root-a".into(),
            query: serde_json::from_value(json!({"kind":"findings","source":"model_analysis"}))
                .unwrap(),
        };
        let value = json!({"scope":"profile-a","model_id":"one","history":history,
            "model_revision":"rev:one","engineering_input_root":"root-a",
            "kind":"findings","source":"model_analysis"});
        assert!(accept(
            operation(),
            value.clone(),
            history.clone(),
            history.clone()
        ));
        for (field, crossed) in [
            ("scope", "profile-b"),
            ("engineering_input_root", "root-b"),
            ("source", "profile"),
            ("kind", "usage"),
            ("model_revision", "rev:two"),
        ] {
            let mut changed = value.clone();
            changed[field] = json!(crossed);
            assert!(
                !accept(operation(), changed, history.clone(), history.clone()),
                "{field}"
            );
        }
        let mut previous = history.clone();
        previous["history_pin"] = json!("other");
        assert!(!accept(operation(), value, previous, history));
    }
}
