//! Display casing for transformer names in plans. Engineering identities stay exact.
use std::collections::BTreeMap;

/// Python `str.title()` casing for canonical ASCII transformer identifiers: a cased run starts with uppercase and
/// continues lowercase; an uncased character (including `_`, digits and
/// apostrophes) starts a new run. Separators and whitespace are preserved.
pub fn transformer_title(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    let mut previous_cased = false;
    for ch in name.chars() {
        let cased = ch.is_lowercase() || ch.is_uppercase();
        if previous_cased {
            out.extend(ch.to_lowercase());
        } else {
            out.extend(ch.to_uppercase());
        }
        previous_cased = cased;
    }
    out
}

/// Transformer title and legacy aliases that contain that same name.
/// Authored titles/drawing references with different words remain authored.
pub fn plan_text(text: &mut BTreeMap<String, String>) {
    let Some(identity) = text.get("transformer").cloned() else {
        return;
    };
    for key in ["transformer", "title", "drawing"] {
        if text.get(key) == Some(&identity) {
            text.insert(key.into(), transformer_title(&identity));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn title_matches_word_boundaries_without_renaming_engineering_keys() {
        for (raw, shown) in [
            ("agahehe", "Agahehe"),
            ("upgrade_munyinya", "Upgrade_Munyinya"),
            ("CYIMPUNGA-TR02a", "Cyimpunga-Tr02A"),
            ("ruko's site", "Ruko'S Site"),
            ("  éCOLE  nord", "  École  Nord"),
        ] {
            assert_eq!(transformer_title(raw), shown);
        }
        let mut text = BTreeMap::from([
            ("transformer".into(), "agahehe".into()),
            ("title".into(), "agahehe".into()),
            ("drawing".into(), "LV-2026-017".into()),
            ("project".into(), "czgmdwth_gisagara".into()),
        ]);
        plan_text(&mut text);
        assert_eq!(text["transformer"], "Agahehe");
        assert_eq!(text["title"], "Agahehe");
        assert_eq!(text["drawing"], "LV-2026-017");
        assert_eq!(text["project"], "czgmdwth_gisagara");
    }
}
