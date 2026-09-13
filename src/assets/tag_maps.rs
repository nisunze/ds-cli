//! One read-only index for maps representing tag groups. A city is a tag value,
//! not a second map store. Index entries retain the producer's asset identity.
use super::Link;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

pub const FAMILY: &str = "tag-group-map";

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Tag {
    pub definition_id: String,
    pub value: String,
}

/// The old Solar role is retained as a consumer alias of the general class.
pub fn is_group_map(role: &str) -> bool {
    matches!(role, "city_map" | "tag_group_map")
}

pub fn tags(links: &[Link]) -> Vec<Tag> {
    links
        .iter()
        .filter_map(|link| match link {
            Link::Tag {
                definition_id,
                value,
            } => Some(Tag {
                definition_id: definition_id.clone(),
                value: value.clone(),
            }),
            _ => None,
        })
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

/// Index exactly one authorized catalogue page. Multiple group memberships
/// reference a single entry in `maps`; pagination never claims a complete set.
pub fn index(project: &str, page: &Value) -> Result<Value, String> {
    let rows = page["assets"]
        .as_array()
        .ok_or("Asset page is missing its rows")?;
    if rows.len() > 200 || project.is_empty() {
        return Err("Tag map index requires one project and at most 200 asset rows".into());
    }
    let more = page["has_more"]
        .as_bool()
        .ok_or("Asset page lacks completeness evidence")?;
    let truncated = page["truncated"]
        .as_bool()
        .ok_or("Asset page lacks scan evidence")?;
    let cursor = page
        .get("next_cursor")
        .and_then(Value::as_str)
        .unwrap_or("");
    if (more || truncated) && cursor.is_empty() {
        return Err("Incomplete asset page lacks a continuation cursor".into());
    }
    let mut maps = BTreeMap::new();
    let mut groups: BTreeMap<Tag, BTreeSet<String>> = BTreeMap::new();
    for asset in rows {
        if asset["project_id"] != project {
            return Err("Asset page contains another project".into());
        }
        let role = asset["reference"]["role"].as_str().unwrap_or("");
        if !is_group_map(role) {
            continue;
        }
        let id = asset["asset_id"]
            .as_str()
            .filter(|s| !s.is_empty())
            .ok_or("Map lacks asset identity")?;
        let digest = asset["digest"].as_str().ok_or("Map lacks a byte digest")?;
        if !digest.strip_prefix("sha256:").is_some_and(|d| {
            d.len() == 64
                && d.bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        }) {
            return Err("Map has an invalid byte digest".into());
        }
        let links: Vec<Link> =
            serde_json::from_value(asset["links"].clone()).map_err(|_| "Map has invalid links")?;
        let tags = tags(&links);
        if tags.is_empty() {
            return Err("Tag group map has no tag membership".into());
        }
        let item = json!({"asset_id":id,"name":asset["name"],"family":FAMILY,
            "digest":digest,"format":asset["format"],"bytes":asset["bytes"],
            "status":asset["status"],"reference":asset["reference"],"tags":tags});
        if maps.get(id).is_some_and(|prior| prior != &item) {
            return Err("Conflicting map asset identities".into());
        }
        maps.insert(id.to_string(), item);
        for tag in tags {
            groups.entry(tag).or_default().insert(id.to_string());
        }
    }
    Ok(
        json!({"schema":"ds.assets.tag-map-index/v1","project_id":project,"family":FAMILY,
        "maps":maps.into_values().collect::<Vec<_>>(),
        "groups":groups.into_iter().map(|(tag, ids)| json!({"tag":tag,"label":tag.value,"asset_ids":ids})).collect::<Vec<_>>(),
        "more":more || truncated,"truncated":truncated,"next_cursor":cursor,"storage_copied":false}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    fn page() -> Value {
        json!({"assets":[{"project_id":"p","asset_id":"a_123456789abc","name":"Gagal — MT A3",
            "reference":{"work_id":"work","output_id":"map.png","role":"city_map"},
            "digest":format!("sha256:{}","a".repeat(64)),"format":"png","bytes":100,"status":"durable",
            "links":[{"kind":"tag","definition_id":"city","value":"Gagal"},{"kind":"tag","definition_id":"phase","value":"II"}]}],
            "has_more":false,"truncated":false})
    }
    #[test]
    fn group_memberships_share_one_artifact_and_keep_authoritative_identity() {
        let output = index("p", &page()).unwrap();
        assert_eq!(output["maps"].as_array().unwrap().len(), 1);
        assert_eq!(output["groups"].as_array().unwrap().len(), 2);
        assert_eq!(
            output["groups"][0]["asset_ids"][0],
            output["groups"][1]["asset_ids"][0]
        );
        assert_eq!(output["maps"][0]["reference"]["output_id"], "map.png");
        assert_eq!(output["storage_copied"], false);
    }
    #[test]
    fn incomplete_and_foreign_pages_never_look_complete() {
        let mut p = page();
        p["has_more"] = json!(true);
        assert!(index("p", &p).is_err());
        p["next_cursor"] = json!("opaque");
        assert_eq!(index("p", &p).unwrap()["more"], true);
        assert!(index("other", &p).is_err());
        p["assets"][0]["links"] = json!([]);
        assert!(index("p", &p).is_err());
    }
}
