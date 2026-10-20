//! Navigation searches model-owned labels and stable structure identities.
use crate::model::Scene;
use std::collections::BTreeMap;
#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    pub id: String,
    pub number: Option<String>,
    pub name: String,
    pub station: f64,
    pub rank: u8,
}
pub fn search(scene: &Scene, numbers: &BTreeMap<String, String>, query: &str) -> Vec<Entry> {
    let query = query.trim();
    let lowered = query.to_lowercase();
    let mut entries = BTreeMap::new();
    for s in scene.bands.iter().flat_map(|b| &b.structures) {
        let number = numbers
            .get(&s.entity_id)
            .cloned()
            .or_else(|| s.engineering_number.clone());
        let n = number.as_deref().unwrap_or("").trim();
        let rank = if query.is_empty() {
            Some(2)
        } else if n == query {
            Some(0)
        } else if !n.is_empty() && n.starts_with(query) {
            Some(1)
        } else if s.engineering_name.to_lowercase().contains(&lowered) {
            Some(2)
        } else {
            None
        };
        if let Some(rank) = rank {
            entries.entry(s.entity_id.clone()).or_insert(Entry {
                id: s.entity_id.clone(),
                number,
                name: s.engineering_name.clone(),
                station: s.ground.scene_x,
                rank,
            });
        }
    }
    let mut entries: Vec<_> = entries.into_values().collect();
    entries.sort_by(|a, b| {
        a.rank
            .cmp(&b.rank)
            .then(a.station.total_cmp(&b.station))
            .then(a.id.cmp(&b.id))
    });
    entries
}
