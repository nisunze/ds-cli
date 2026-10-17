//! Read and clear canonical task geometry and its retained object links.
pub mod clear;
pub mod read;
use ds_command_kernel::task_geometry;
use serde_json::Value;

/// A link's authored wire fields and nothing else — the kernel's one list,
/// which includes an exact DS Grid model pin's `model_version` and
/// `model_digest`: the command decoder refuses an unknown field, a link the
/// graph answers may one day carry a projection the engine did not author,
/// and a pin re-sent without its fields would no longer say what it pinned.
pub fn wire_link(link: &Value) -> Value {
    ds_command_kernel::project_management::model_links::wire_link(link)
}

/// The task's `ds_object` links this family attached — the trace of a
/// geometry — as opposed to the links a person attached by hand.
pub fn is_geometry_link(link: &Value) -> bool {
    link["kind"] == "ds_object"
        && matches!(
            link["object_type"].as_str(),
            Some(task_geometry::OBJECT_TYPE_STRUCTURE) | Some(task_geometry::OBJECT_TYPE_ALIGNMENT)
        )
}

/// The task's links as the server published them, or none.
pub fn existing_links(task: &Value) -> Vec<Value> {
    task["links"].as_array().cloned().unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_geometry_link_is_told_from_a_hand_attached_one_and_re_sent_with_its_wire_fields() {
        assert!(is_geometry_link(
            &json!({ "kind": "ds_object", "object_type": "dsgrid_structure" })
        ));
        assert!(is_geometry_link(
            &json!({ "kind": "ds_object", "object_type": "dsgrid_alignment" })
        ));
        assert!(!is_geometry_link(
            &json!({ "kind": "ds_object", "object_type": "transformer" })
        ));
        assert!(!is_geometry_link(
            &json!({ "kind": "survey_entry", "object_type": "dsgrid_structure" })
        ));
        // The decoder refuses an unknown field, so a projection the graph
        // added would sink the whole command.
        assert_eq!(
            wire_link(
                &json!({ "kind": "pm_task", "project_id": "p", "entity_id": "t-0", "extra": "projection", "label": null })
            ),
            json!({ "kind": "pm_task", "project_id": "p", "entity_id": "t-0" })
        );
    }
}
