//! Entry editing and connectivity projections shared by every Survey host.
use serde_json::{json, Value};
const MEDIA: &[&str] = &[
    "photo",
    "photos",
    "video",
    "videos",
    "audio",
    "audios",
    "document",
    "documents",
    "signature",
];
fn text(value: &Value) -> &str {
    value.as_str().unwrap_or("").trim()
}
fn props(value: &Value) -> &Value {
    value.get("properties").unwrap_or(value)
}
pub(super) fn evaluate(r: &Value) -> Result<Value, String> {
    match r["mode"].as_str() {
        Some("media_type") => Ok(json!(MEDIA.contains(&text(&r["field_type"])))),
        Some("editable_fields") => {
            let fields = r["form"]["fields"].as_array();
            if fields.is_some_and(|f| f.len() > super::MAX_SURVEY_FIELDS) {
                return Err("Survey form exceeds its field bound".into());
            }
            Ok(json!(fields
                .into_iter()
                .flatten()
                .filter(|f| {
                    let kind = text(&f["type"]);
                    !kind.is_empty()
                        && !MEDIA.contains(&kind)
                        && !["compound", "kv", "textBlock", "separator"].contains(&kind)
                })
                .collect::<Vec<_>>()))
        }
        Some("edit") => {
            let f = &r["feature"];
            let p = props(f);
            let form = &r["form"];
            let is_survey = !text(&f["dsProject"]).is_empty()
                && !text(&f["formSlug"]).is_empty()
                && !text(&f["docId"]).is_empty()
                && ["__ds_design_layer", "_sketches_layer", "_sketches_id"]
                    .iter()
                    .all(|key| {
                        p[*key].is_null() || p[*key].as_str().is_some_and(|v| v.trim().is_empty())
                    });
            let editable = evaluate(&json!({"mode":"editable_fields","form":form}))?;
            let edge =
                super::policy::evaluate(&json!({"mode":"derived_edge","form":form}))? == true;
            let reason = if !is_survey {
                "This is not a survey entry."
            } else if form.is_null() {
                "Survey form schema is not loaded."
            } else if form["permissions"]["can_edit"] == false {
                "You do not have permission to edit this form."
            } else if edge {
                "Edges are edited through connected node workflows."
            } else if editable.as_array().is_none_or(|f| f.is_empty()) {
                "This form has no editable fields."
            } else if r["grants"]["entries.edit_all"] != true
                && r["grants"]["entries.edit_own"] != true
            {
                "Requires entries.edit_all or entries.edit_own."
            } else if p["can_edit_this_entry"] == false {
                "You can only edit entries you created."
            } else if p["can_edit_this_entry"] != true {
                "Reload this entry before editing it."
            } else {
                "This survey entry cannot be edited."
            };
            Ok(
                json!({"is_survey":is_survey,"can_edit":reason == "This survey entry cannot be edited.",
                "can_delete":is_survey && r["grants"]["entries.delete"] == true,"reason":reason}),
            )
        }
        Some("edges") => {
            let rows = r["rows"].as_array().ok_or("Edge rows are required")?;
            if rows.len() > 256 {
                return Err("Edge search batch exceeds 256 rows".into());
            }
            let node = &r["node_id"];
            let filter = &r["match"];
            let mut found = Vec::new();
            for row in rows {
                let f = &row["feature"];
                let p = props(f);
                let c = &p["connectivity"];
                if p["is_deleted"] == true
                    || p["metadata"]["is_deleted"] == true
                    || !["LineString", "MultiLineString"].contains(&text(&f["geometry"]["type"]))
                    || !c.is_object()
                {
                    continue;
                }
                let a = if !filter["precedent"].is_null() {
                    c["precedent"] == filter["precedent"]
                } else {
                    filter.is_null() && c["precedent"] == *node
                };
                let b = if !filter["antecedent"].is_null() {
                    c["antecedent"] == filter["antecedent"]
                } else {
                    filter.is_null() && c["antecedent"] == *node
                };
                let matches = if !filter["precedent"].is_null() && !filter["antecedent"].is_null() {
                    a && b
                } else {
                    a || b
                };
                if !matches {
                    continue;
                }
                let id = [
                    p.get("doc_id"),
                    p.get("id"),
                    p.get("feature_id"),
                    f.get("id"),
                ]
                .into_iter()
                .flatten()
                .find(|v| !text(v).is_empty());
                if let Some(id) = id {
                    found.push(if r["projection"] == "rows" {
                        row.clone()
                    } else {
                        json!({"formSlug":row["form_slug"],"docId":id})
                    });
                }
            }
            Ok(json!(found))
        }
        Some("precedent_change") => {
            let coords = if r["geometry"]["type"] == "Point" {
                r["geometry"]["coordinates"].clone()
            } else {
                Value::Null
            };
            let mut connectivity = props(&r["feature"])["connectivity"]
                .as_object()
                .cloned()
                .unwrap_or_default();
            connectivity.insert("precedent".into(), r["precedent"]["docId"].clone());
            if !text(&r["precedent"]["formSlug"]).is_empty() {
                connectivity.insert(
                    "precedent_form_id".into(),
                    r["precedent"]["formSlug"].clone(),
                );
            }
            if !coords.is_null() {
                let a = super::entry::point(&r["precedent"]["coordinates"])?;
                let b = super::entry::point(&coords)?;
                connectivity.insert(
                    "distance_m".into(),
                    json!(ds_geo_lite::haversine_m(a.0, a.1, b.0, b.1)),
                );
            }
            connectivity.remove("length_m");
            Ok(json!({"coordinates":coords,"connectivity":connectivity}))
        }
        Some("edge_context") => {
            if r["form"].is_null() {
                return Ok(Value::Null);
            }
            let p = &r["precedent"];
            super::entry::point(&p["coordinates"])?;
            super::entry::point(&r["node_coordinates"])?;
            Ok(
                json!({"projectId":r["project_id"],"formSlug":r["form_slug"],"schema":r["form"],
                "geometryType":r["form"]["drawing_settings"].get("geometry_type").unwrap_or(&json!("LineString")),
                "geometry":{"type":"LineString","coordinates":[p["coordinates"],r["node_coordinates"]]},
                "connectivity":{"precedentDocId":p["docId"],"precedentFormId":p["formSlug"],"precedentCoordinates":p["coordinates"],"antecedentDocId":r["node_id"],"antecedentFormId":r["node_form"]}}),
            )
        }
        _ => Err("Unknown Survey access command".into()),
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn edge_candidate_projection_keeps_only_matching_rows() {
        let edge = json!({"form_slug":"lines","feature":{"id":"e","geometry":{"type":"LineString"},"properties":{"metadata":{"client_version":2},"connectivity":{"precedent":"n"}}}});
        let other = json!({"form_slug":"lines","feature":{"id":"other","geometry":{"type":"LineString"},"properties":{"connectivity":{"precedent":"other"}}}});
        assert_eq!(evaluate(&json!({"mode":"edges","node_id":"n","rows":[edge.clone(),other],"projection":"rows"})).unwrap(),json!([edge]));
    }
    #[test]
    fn deleted_edges_and_nodes_are_never_cascade_targets() {
        let edge = json!({"form_slug":"lines","feature":{"id":"e","geometry":{"type":"LineString"},"properties":{"connectivity":{"precedent":"n"}}}});
        let mut deleted = edge.clone();
        deleted["feature"]["properties"]["metadata"] = json!({"is_deleted":true});
        let mut node = edge.clone();
        node["feature"]["geometry"]["type"] = json!("Point");
        assert_eq!(
            evaluate(&json!({"mode":"edges","node_id":"n","rows":[edge,deleted,node]})).unwrap(),
            json!([{"formSlug":"lines","docId":"e"}])
        );
    }
    #[test]
    fn absent_server_grant_refuses_edit_even_with_edit_all() {
        let r = json!({"mode":"edit","feature":{"dsProject":"p","formSlug":"f","docId":"d","properties":{}},"form":{"fields":[{"type":"text"}]},"grants":{"entries.edit_all":true}});
        assert_eq!(evaluate(&r).unwrap()["can_edit"], false);
        let mut allowed = r;
        allowed["feature"]["properties"]["can_edit_this_entry"] = json!(true);
        assert_eq!(evaluate(&allowed).unwrap()["can_edit"], true);
    }
}
