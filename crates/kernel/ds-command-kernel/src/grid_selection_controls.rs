//! Shared native selection preferences, query admission and ordered composition.
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeSet;
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Predicate {
    Intersects,
    #[default]
    Within,
    Outside,
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    #[default]
    Replace,
    Add,
    Remove,
    Intersect,
}
pub use ds_profile_policy::profile_selection::{profile_families, Family};
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Space {
    Plan,
    Profile,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct State {
    pub predicate: Predicate,
    pub mode: Mode,
    pub families: Option<Vec<Family>>,
}
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Patch {
    pub predicate: Option<Predicate>,
    pub mode: Option<Mode>,
    #[serde(default, deserialize_with = "families_patch")]
    pub families: Option<Option<Vec<Family>>>,
}
fn families_patch<'de, D: serde::Deserializer<'de>>(
    d: D,
) -> Result<Option<Option<Vec<Family>>>, D::Error> {
    Option::<Vec<Family>>::deserialize(d).map(Some)
}
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum Action {
    Read,
    Configure {
        patch: Patch,
    },
    Admit {
        query: Value,
    },
    Compose {
        mode: Mode,
        current: Vec<String>,
        hits: Vec<String>,
    },
}
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub scope: String,
    pub space: Space,
    pub available_families: Vec<Family>,
    pub request: Action,
}
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Query {
    model_id: String,
    expected_revision: String,
    space: Space,
    polygon: Value,
    #[serde(default)]
    predicate: Option<Predicate>,
    #[serde(default)]
    mode: Option<Mode>,
    #[serde(default)]
    families: Option<Vec<Family>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    filter: Option<Value>,
}
fn invalid(message: &str) -> String {
    format!("invalid_grid_lasso: {message}")
}
fn id(value: &str) -> Result<(), String> {
    if value.is_empty()
        || value.trim() != value
        || value.len() > 200
        || value.chars().any(char::is_control)
    {
        return Err(invalid("use exact nonblank model/revision IDs <=200 bytes"));
    }
    Ok(())
}
fn families(values: &[Family]) -> Result<(), String> {
    if values.len() > 5 || values.iter().collect::<BTreeSet<_>>().len() != values.len() {
        return Err(invalid("choose distinct native layers"));
    }
    Ok(())
}
pub fn execute(state: &State, request: &Request) -> Result<(State, Value), String> {
    if request.scope.is_empty() || request.scope.len() > 1024 {
        return Err(invalid("invalid selection scope"));
    }
    families(&request.available_families)?;
    let available: Vec<_> = request
        .available_families
        .iter()
        .copied()
        .filter(|v| request.space != Space::Plan || *v != Family::AttachmentPoints)
        .collect();
    let mut next = state.clone();
    let resolved = |state: &State| {
        available
            .iter()
            .copied()
            .filter(|v| state.families.as_ref().is_none_or(|p| p.contains(v)))
            .collect::<Vec<_>>()
    };
    let mut result = json!({});
    match &request.request {
        Action::Read => {}
        Action::Configure { patch } => {
            if let Some(v) = patch.predicate {
                next.predicate = v;
            }
            if let Some(v) = patch.mode {
                next.mode = v;
            }
            if let Some(v) = &patch.families {
                if let Some(v) = v {
                    families(v)?;
                }
                next.families = v.clone();
            }
        }
        Action::Admit { query } => {
            if serde_json::to_vec(query)
                .map_err(|e| invalid(&e.to_string()))?
                .len()
                > 65536
            {
                return Err(invalid("query exceeds 64 KiB"));
            }
            let mut query: Query =
                serde_json::from_value(query.clone()).map_err(|e| invalid(&e.to_string()))?;
            id(&query.model_id)?;
            id(&query.expected_revision)?;
            if query.space != request.space {
                return Err(invalid("selection space differs"));
            }
            let ring = if query.polygon.is_array() {
                if query.space == Space::Plan {
                    return Err(invalid("Plan requires WGS84 GeoJSON Polygon"));
                }
                &query.polygon
            } else {
                let object = query
                    .polygon
                    .as_object()
                    .ok_or_else(|| invalid("use Polygon geometry"))?;
                if object.len() != 2
                    || object.get("type") != Some(&json!("Polygon"))
                    || object
                        .get("coordinates")
                        .and_then(Value::as_array)
                        .is_none_or(|v| v.len() != 1)
                {
                    return Err(invalid("use one Polygon ring without extra keys or holes"));
                }
                &query.polygon["coordinates"][0]
            };
            let ring: Vec<[f64; 2]> = serde_json::from_value(ring.clone())
                .map_err(|_| invalid("use finite exact coordinate pairs"))?;
            let closed = ring.first() == ring.last();
            if !(3..=256).contains(&ring.len())
                || (!query.polygon.is_array() && (!closed || ring.len() < 4))
                || ring.iter().any(|p| {
                    p.iter().any(|v| !v.is_finite())
                        || if query.space == Space::Plan {
                            p[0].abs() > 180.0 || p[1].abs() > 90.0
                        } else {
                            p.iter().any(|v| v.abs() > 1e9)
                        }
                })
            {
                return Err(invalid("polygon is outside the admitted bounds"));
            }
            if let Some(v) = query.predicate {
                next.predicate = v;
            }
            if let Some(v) = query.mode {
                next.mode = v;
            }
            if let Some(v) = &query.families {
                families(v)?;
                if v.iter().any(|v| !available.contains(v)) {
                    return Err(invalid("requested layer is unavailable in the held view"));
                }
                next.families = Some(v.clone());
            }
            query.predicate = Some(next.predicate);
            query.mode = Some(next.mode);
            query.families = Some(resolved(&next));
            if query.space == Space::Profile && !query.polygon.is_array() {
                query.polygon = query.polygon["coordinates"][0].clone();
            }
            result["query"] = json!(query);
            // Admission resolves a one-shot request without changing preferences.
            next = state.clone();
        }
        Action::Compose {
            mode,
            current,
            hits,
        } => {
            if current.len() > 100000
                || hits.len() > 100000
                || current
                    .iter()
                    .chain(hits)
                    .any(|v| v.is_empty() || v.len() > 1024)
            {
                return Err(invalid("selection identities exceed native bounds"));
            }
            let hit_set = hits.iter().collect::<BTreeSet<_>>();
            let source: Vec<_> = match mode {
                Mode::Replace => hits.iter().collect(),
                Mode::Add => current.iter().chain(hits).collect(),
                Mode::Remove => current.iter().filter(|v| !hit_set.contains(v)).collect(),
                Mode::Intersect => current.iter().filter(|v| hit_set.contains(v)).collect(),
            };
            let mut seen = BTreeSet::new();
            let ids = source
                .into_iter()
                .filter(|v| seen.insert(*v))
                .collect::<Vec<_>>();
            if ids.len() > 100000 {
                return Err(invalid("combined selection exceeds 100000 identities"));
            }
            result["entity_ids"] = json!(ids);
            result["primary"] = json!(ids.last());
        }
    }
    result["state"] = json!(next);
    result["resolved_families"] = json!(resolved(&next));
    result["available_families"] = json!(available);
    result["layers"] = json!(available.iter().map(|v|json!({"value":v,"label":match v {Family::Structures=>"Structures",Family::TerrainPoints=>"Terrain points",Family::Alignments=>"Alignments",Family::TensionSections=>"Tension sections",Family::AttachmentPoints=>"Attachment points"}})).collect::<Vec<_>>());
    result["predicates"] = json!([{"value":"within","label":"Within"},{"value":"intersects","label":"Intersects"},{"value":"outside","label":"Outside"}]);
    result["modes"] = json!([{"value":"replace","label":"Replace"},{"value":"add","label":"Add"},{"value":"remove","label":"Remove"},{"value":"intersect","label":"Keep intersection"}]);
    Ok((next, result))
}
#[cfg(test)]
#[path = "grid_selection_controls_tests.rs"]
mod tests;
