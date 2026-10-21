//! One Settings sheet copied from another project's configuration.
//!
//! A whole-document copy (ds-brain `network_documents`) replaces every sheet a
//! project holds. This is the narrower door: one admitted sheet taken from a
//! source project's freshly read configuration into a destination's, planned
//! first and applied only under the reviewed plan's digest.
//!
//! The sheet's identity is admitted by the source configuration itself: the
//! key must be a sheet the source holds AND one its configuration groups
//! declare (the server's closed sheet registry, as read). The value then
//! passes exactly the validation an edit of that sheet passes ([`save_sheet`]):
//! against the destination's own sheet when it holds one, otherwise against
//! the source's model for it. Only that sheet is written; every other
//! destination sheet is untouched. [`save_sheet`] itself keeps refusing a
//! sheet the document does not hold — an absent sheet is created only here.
//!
//! No IO, clock or credential: hosts read both documents, and the write, the
//! source READ and destination WRITE authority stay with the gateway.
use super::{save_sheet, settings_equal};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

pub const SHEET_COPY_SCHEMA: &str = "ds.design-config-sheet-copy-plan/v1";

const CREATE: &str = "create";
const REPLACE: &str = "replace";
const IDENTICAL: &str = "identical";

/// The reviewed plan and the exact sheet value its apply writes.
#[derive(Debug, Clone, PartialEq)]
pub struct SheetCopy {
    pub plan: Value,
    pub value: Value,
}

/// Decide copying `sheet` from `source` (project `source_project`) into
/// `destination` (project `project`). Both documents are configuration reads
/// (`{groups, sheets, models}`).
pub fn plan_sheet_copy(
    project: &str,
    source_project: &str,
    source: &Value,
    destination: &Value,
    sheet: &str,
) -> Result<SheetCopy, &'static str> {
    let project = project.trim();
    let source_project = source_project.trim();
    if project.is_empty() || source_project.is_empty() || project == source_project {
        return Err("config_copy_invalid");
    }
    if sheet.is_empty() || sheet.len() > 128 {
        return Err("config_sheet_unknown");
    }
    if !source["sheets"].is_object() || !destination["sheets"].is_object() {
        return Err("config_document_invalid");
    }
    let source_value = source["sheets"].get(sheet).ok_or("config_sheet_unknown")?;
    let group = admitted_group(source, sheet).ok_or("config_sheet_not_admitted")?;
    let current = destination["sheets"].get(sheet);
    let value = match current {
        Some(_) => save_sheet(destination, sheet, source_value)?,
        None => {
            let mut staged = destination.clone();
            staged["sheets"][sheet] = source_value.clone();
            if let Some(model) = source["models"].get(sheet) {
                if staged["models"].get(sheet).is_none() {
                    staged["models"][sheet] = model.clone();
                }
            }
            save_sheet(&staged, sheet, source_value)?
        }
    };
    let outcome = match current {
        None => CREATE,
        Some(current) if settings_equal(current, &value) => IDENTICAL,
        Some(_) => REPLACE,
    };
    let source_state = json!({"sha256": digest(&value), "rows": rows(&value)});
    let destination_state = match current {
        Some(current) => json!({"present": true, "sha256": digest(current), "rows": rows(current)}),
        None => json!({"present": false, "sha256": null, "rows": 0}),
    };
    let fenced = json!({
        "schema": SHEET_COPY_SCHEMA,
        "project": project,
        "source_project": source_project,
        "sheet": sheet,
        "outcome": outcome,
        "source": source_state,
        "destination": destination_state,
    });
    let plan_sha256 = digest(&fenced);
    let preserved = destination["sheets"].as_object().map_or(0, |sheets| {
        sheets.keys().filter(|key| *key != sheet).count()
    });
    let mut plan = fenced;
    plan["group"] = json!(group);
    plan["preserved_sheets"] = json!(preserved);
    plan["plan_sha256"] = json!(plan_sha256);
    Ok(SheetCopy { plan, value })
}

/// Re-decide the copy from fresh reads and return the value to write only when
/// it is still the reviewed plan.
pub fn copy_sheet(
    project: &str,
    source_project: &str,
    source: &Value,
    destination: &Value,
    sheet: &str,
    expected_plan_sha256: &str,
) -> Result<Value, &'static str> {
    let decided = plan_sheet_copy(project, source_project, source, destination, sheet)?;
    if decided.plan["plan_sha256"] != expected_plan_sha256 {
        return Err("config_copy_plan_changed");
    }
    Ok(decided.value)
}

fn admitted_group(document: &Value, sheet: &str) -> Option<String> {
    document["groups"]
        .as_object()?
        .iter()
        .find_map(|(group, declared)| {
            declared["sheets"]
                .as_array()?
                .iter()
                .any(|key| key == sheet)
                .then(|| group.clone())
        })
}

fn rows(value: &Value) -> usize {
    match value {
        Value::Array(rows) => rows.len(),
        Value::Object(sets) => sets
            .values()
            .map(|set| {
                set.as_array()
                    .or_else(|| set.get("table").and_then(Value::as_array))
                    .map_or(0, Vec::len)
            })
            .sum(),
        _ => 0,
    }
}

/// Key-order independent digest, so a server's map ordering never moves a plan.
fn digest(value: &Value) -> String {
    fn canonical(value: &Value, out: &mut String) {
        match value {
            Value::Object(map) => {
                let mut keys: Vec<&String> = map.keys().collect();
                keys.sort();
                out.push('{');
                for (index, key) in keys.into_iter().enumerate() {
                    if index > 0 {
                        out.push(',');
                    }
                    out.push_str(&Value::String(key.clone()).to_string());
                    out.push(':');
                    canonical(&map[key], out);
                }
                out.push('}');
            }
            Value::Array(items) => {
                out.push('[');
                for (index, item) in items.iter().enumerate() {
                    if index > 0 {
                        out.push(',');
                    }
                    canonical(item, out);
                }
                out.push(']');
            }
            other => out.push_str(&other.to_string()),
        }
    }
    let mut text = String::new();
    canonical(value, &mut text);
    format!("{:x}", Sha256::digest(text.as_bytes()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source() -> Value {
        json!({
            "groups": {
                "header_mapping": {"label": "Header Mapping", "sheets": ["layer_mapping"]},
                "data_cleaning": {"label": "Data Cleaning", "sheets": ["cust_category"]},
                "parameters": {"label": "Parameters", "sheets": ["project_settings"]}
            },
            "sheets": {
                "layer_mapping": [{"layer": "lv_poles", "source": "Poles"}, {}],
                "cust_category": [{"clean_name": "household", "misspelled_names": ["hh"]}],
                "project_settings": [{"parameter": "Max Span", "value": "45"}],
                "stray": []
            },
            "models": {}
        })
    }

    fn destination() -> Value {
        json!({
            "groups": {"data_cleaning": {"label": "Data Cleaning", "sheets": ["cust_category"]}},
            "sheets": {
                "cust_category": [{"clean_name": "household"}],
                "transformer": [{"layer": "tx"}]
            }
        })
    }

    #[test]
    fn an_absent_destination_sheet_is_created_alone_and_the_edit_door_still_refuses_it() {
        assert_eq!(
            save_sheet(
                &destination(),
                "layer_mapping",
                &source()["sheets"]["layer_mapping"]
            ),
            Err("config_sheet_unknown")
        );
        let copy =
            plan_sheet_copy("dst", "src", &source(), &destination(), "layer_mapping").unwrap();
        assert_eq!(copy.plan["outcome"], CREATE);
        assert_eq!(copy.plan["group"], "header_mapping");
        assert_eq!(copy.plan["destination"]["present"], false);
        assert_eq!(copy.plan["preserved_sheets"], 2);
        assert_eq!(
            copy.value,
            json!([{"layer": "lv_poles", "source": "Poles"}])
        );
        assert_eq!(copy.plan["source"]["rows"], 1);
    }

    #[test]
    fn an_existing_sheet_copy_replaces_then_reads_identical() {
        let copy =
            plan_sheet_copy("dst", "src", &source(), &destination(), "cust_category").unwrap();
        assert_eq!(copy.plan["outcome"], REPLACE);
        assert_eq!(copy.plan["destination"]["present"], true);
        let mut written = destination();
        written["sheets"]["cust_category"] = copy.value.clone();
        let again = plan_sheet_copy("dst", "src", &source(), &written, "cust_category").unwrap();
        assert_eq!(again.plan["outcome"], IDENTICAL);
        assert_ne!(again.plan["plan_sha256"], copy.plan["plan_sha256"]);
    }

    #[test]
    fn parameter_sheets_take_the_edit_coercion() {
        let copy =
            plan_sheet_copy("dst", "src", &source(), &destination(), "project_settings").unwrap();
        assert_eq!(copy.value, json!([{"parameter": "max_span", "value": 45}]));
    }

    #[test]
    fn unknown_unadmitted_and_malformed_copies_refuse() {
        let (s, d) = (source(), destination());
        assert_eq!(
            plan_sheet_copy("dst", "src", &s, &d, "absent_everywhere").unwrap_err(),
            "config_sheet_unknown"
        );
        assert_eq!(
            plan_sheet_copy("dst", "src", &s, &d, "stray").unwrap_err(),
            "config_sheet_not_admitted"
        );
        assert_eq!(
            plan_sheet_copy("dst", "dst", &s, &d, "layer_mapping").unwrap_err(),
            "config_copy_invalid"
        );
        assert_eq!(
            plan_sheet_copy("dst", "src", &json!({}), &d, "layer_mapping").unwrap_err(),
            "config_document_invalid"
        );
        let mut shaped = s.clone();
        shaped["sheets"]["layer_mapping"] = json!({"not": "rows"});
        assert_eq!(
            plan_sheet_copy("dst", "src", &shaped, &d, "layer_mapping").unwrap_err(),
            "config_sheet_shape_invalid"
        );
    }

    #[test]
    fn the_json_door_answers_the_same_plan() {
        let request = json!({"op":"sheet_copy_plan","project":"dst","source_project":"src",
            "source":source(),"destination":destination(),"sheet_key":"layer_mapping"});
        let answer: Value = serde_json::from_str(
            &super::super::evaluate(&serde_json::to_vec(&request).unwrap()).unwrap(),
        )
        .unwrap();
        let direct =
            plan_sheet_copy("dst", "src", &source(), &destination(), "layer_mapping").unwrap();
        assert_eq!(answer["result"], direct.plan);
        let unknown = json!({"op":"sheet_copy_plan","project":"dst","source_project":"src",
            "source":source(),"destination":destination(),"sheet_key":"nope"});
        assert_eq!(
            super::super::evaluate(&serde_json::to_vec(&unknown).unwrap()).unwrap_err(),
            "config_sheet_unknown"
        );
    }

    #[test]
    fn apply_is_fenced_on_the_reviewed_plan_and_key_order_never_moves_it() {
        let (s, d) = (source(), destination());
        let copy = plan_sheet_copy("dst", "src", &s, &d, "layer_mapping").unwrap();
        let reviewed = copy.plan["plan_sha256"].as_str().unwrap();
        assert_eq!(
            copy_sheet("dst", "src", &s, &d, "layer_mapping", reviewed).unwrap(),
            copy.value
        );
        let mut moved = s.clone();
        moved["sheets"]["layer_mapping"][0]["source"] = json!("Pylons");
        assert_eq!(
            copy_sheet("dst", "src", &moved, &d, "layer_mapping", reviewed).unwrap_err(),
            "config_copy_plan_changed"
        );
        let reordered: Value = serde_json::from_str(
            r#"{"sheets":{"layer_mapping":[{"source":"Poles","layer":"lv_poles"}]},"groups":{"header_mapping":{"sheets":["layer_mapping"]}}}"#,
        )
        .unwrap();
        assert_eq!(
            plan_sheet_copy("dst", "src", &reordered, &d, "layer_mapping")
                .unwrap()
                .plan["plan_sha256"],
            reviewed
        );
    }
}
