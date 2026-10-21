//! Edit receipt transport fences. Network owns commands and history transitions;
//! the client rejects crossed or incomplete acknowledgments.
use super::*;
use ds_network_contracts::profile_control::ProfileControlOperation;

pub(super) fn validate_reply(
    operation: &NativeProfileOperationV1,
    bytes: &[u8],
    model: &str,
    previous: &Value,
    history: &Value,
) -> Result<(), NetworkFailure> {
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
            let operation: ProfileControlOperation =
                serde_json::from_value(operation.clone()).map_err(|e| invalid(e.to_string()))?;
            if value["applied"] != operation.is_mutating()
                || (!operation.is_mutating() && history != previous)
            {
                return Err(invalid(
                    "Profile preview/control changed its admitted mutation contract",
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
}
