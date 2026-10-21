//! One bounded local delete plan. Hosts commit every intent together or none.
//! Held rows are explicit input, not proof of complete server connectivity.
use serde_json::{json, Value};
use std::collections::{BTreeSet, HashMap};
pub const MAX_TARGETS: usize = 500;
pub(super) fn plan(r: &Value) -> Result<Value, String> {
    super::mutation::evaluate(
        &json!({"mode":"authorize","mutation_operation":"delete","grants":r["grants"]}),
    )?;
    let key = r["mutation_id"]
        .as_str()
        .filter(|s| !s.is_empty() && s.len() <= 480 && !s.chars().any(char::is_control))
        .ok_or("Cascade replay identity is required and bounded")?;
    time::OffsetDateTime::parse(
        r["now"].as_str().unwrap_or(""),
        &time::format_description::well_known::Rfc3339,
    )
    .map_err(|_| "Cascade clock must be RFC3339")?;
    let rows = r["rows"]
        .as_array()
        .ok_or("Cascade rows must be an array")?;
    if rows
        .iter()
        .any(|row| row.get("project_id").is_some_and(|p| p != &r["project_id"]))
    {
        return Err("Cascade inventory belongs to another project".into());
    }
    if rows.len() > 10_000 {
        return Err("Cascade inventory exceeds 10000 held entries".into());
    }
    let node = super::mutation::evaluate(&json!({"mode":"identity","payload":{
        "project_id":r["project_id"],"form_id":r["form_id"],"doc_id":r["doc_id"],"operation":"delete"}}))?;
    let mut edges = Vec::new();
    let mut priors = HashMap::new();
    for row in rows {
        let f = &row["feature"];
        let p = f.get("properties").unwrap_or(f);
        let id = [
            p.get("doc_id"),
            p.get("id"),
            p.get("feature_id"),
            f.get("id"),
        ]
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::trim)
        .find(|id| !id.is_empty());
        if let Some(id) = id {
            priors
                .entry((row["form_slug"].as_str().unwrap_or("").trim(), id))
                .or_insert(f);
        }
    }
    for batch in rows.chunks(256) {
        let found = super::access::evaluate(
            &json!({"mode":"edges","node_id":node["doc_id"],"rows":batch,"match":r["match"]}),
        )?;
        edges.extend(found.as_array().unwrap().iter().cloned());
    }
    let mut targets = Vec::new();
    let mut seen = BTreeSet::new();
    if r["edges_only"] != true {
        targets.push(node.clone());
        seen.insert((
            node["form_id"].clone().to_string(),
            node["doc_id"].clone().to_string(),
        ));
    }
    for edge in &edges {
        let identity = super::mutation::evaluate(&json!({"mode":"identity","payload":{
            "project_id":node["project_id"],"form_id":edge["formSlug"],"doc_id":edge["docId"],"operation":"delete"}}))?;
        if seen.insert((
            identity["form_id"].to_string(),
            identity["doc_id"].to_string(),
        )) {
            targets.push(identity);
        }
    }
    if targets.len() > MAX_TARGETS {
        return Err("Cascade exceeds 500 delete targets; no intents were admitted".into());
    }
    let mut intents = Vec::new();
    for (index, target) in targets.iter().enumerate() {
        let prior = if target["form_id"] == node["form_id"] && target["doc_id"] == node["doc_id"] {
            r["prior"].clone()
        } else {
            priors
                .get(&(
                    target["form_id"].as_str().unwrap(),
                    target["doc_id"].as_str().unwrap(),
                ))
                .map(|p| (*p).clone())
                .unwrap_or(Value::Null)
        };
        let intent =
            super::mutation::evaluate(&json!({"mode":"mutate","payload":target,"prior":prior,
            "grants":r["grants"],"user_email":r["user_email"],"now":r["now"],
            "mutation_id":format!("{}:{index}",key)}))?;
        intents.push(json!({"identity":intent["identity"],"payload":intent["payload"]}));
    }
    Ok(
        json!({"schema":"ds.survey-delete-cascade/v1","project_id":node["project_id"],
        "intents":intents,"deleted":targets.len(),"cascaded_edges":targets.len()-usize::from(r["edges_only"] != true),
        "scope":"held_rows","remote_atomic":false}),
    )
}
/// A local preview has no authority to enqueue or send these intents.
pub(super) fn preview(r: &Value) -> Result<Value, String> {
    let mut input = r.clone();
    input["grants"] = json!({"entries.delete":true});
    let mut result = plan(&input)?;
    result["authorized"] = json!(false);
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn request() -> Value {
        json!({"project_id":"p","form_id":"nodes","doc_id":"n","mutation_id":"key","now":"2026-10-02T00:00:00Z","grants":{"entries.delete":true},
        "rows":[{"form_slug":"edges","feature":{"id":"e","geometry":{"type":"LineString"},"properties":{"connectivity":{"precedent":"n"}}}}]})
    }
    #[test]
    fn inventory_can_cross_edge_batch_bound_without_linear_scans_per_target() {
        let mut r = request();
        let edge = r["rows"][0].clone();
        r["rows"] = json!(vec![
            json!({"form_slug":"nodes","feature":{"id":"unrelated","geometry":{"type":"Point"}}});
            600
        ]);
        r["rows"].as_array_mut().unwrap().push(edge);
        assert_eq!(plan(&r).unwrap()["deleted"], 2);
    }
    #[test]
    fn node_and_matching_edges_form_one_deduplicated_project_fenced_plan() {
        let mut r = request();
        let duplicate = r["rows"][0].clone();
        r["rows"].as_array_mut().unwrap().push(duplicate);
        let result = plan(&r).unwrap();
        assert_eq!(result["deleted"], 2);
        assert_eq!(result["intents"][0]["identity"]["doc_id"], "n");
        assert_eq!(result["intents"][1]["payload"]["idempotency_key"], "key:1");
        assert_eq!(result["remote_atomic"], false);
        r["grants"] = json!({});
        assert!(plan(&r).is_err());
    }
    #[test]
    fn oversized_and_malformed_targets_refuse_before_any_admission() {
        let mut r = request();
        r["rows"] = json!((0..501).map(|i| json!({"form_slug":"edges","feature":{"id":format!("e{i}"),"geometry":{"type":"LineString"},"properties":{"connectivity":{"precedent":"n"}}}})).collect::<Vec<_>>());
        assert!(plan(&r).is_err());
        r = request();
        r["rows"][0]["form_slug"] = json!("");
        assert!(plan(&r).is_err());
    }
}
