//! Picker ordering only. Native previews and writes retain type validity.
use crate::panel_model::Definition;

pub fn search<'a>(entries: &'a [Definition], query: &str) -> Vec<&'a Definition> {
    let needle = query.trim().to_lowercase();
    if needle.is_empty() {
        return entries.iter().collect();
    }
    let mut matches: Vec<_> = entries
        .iter()
        .enumerate()
        .filter_map(|(order, entry)| {
            let label = entry.label.to_lowercase();
            let id = entry.entity_id.to_lowercase();
            let rank = if label == needle || id == needle {
                0
            } else if label.starts_with(&needle) || id.starts_with(&needle) {
                1
            } else if label
                .split(|c: char| !c.is_alphanumeric())
                .any(|part| part.starts_with(&needle))
            {
                2
            } else if label.contains(&needle) || id.contains(&needle) {
                3
            } else {
                return None;
            };
            Some((rank, order, entry))
        })
        .collect();
    matches.sort_by_key(|(rank, order, _)| (*rank, *order));
    matches.into_iter().map(|(_, _, entry)| entry).collect()
}
