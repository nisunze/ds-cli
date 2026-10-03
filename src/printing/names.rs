//! Display casing for transformer names in plans. Engineering identities stay exact.
use std::collections::BTreeMap;

/// Declarative display-only transforms; engineering identities remain exact.
#[derive(
    Debug, Clone, Copy, serde::Serialize, serde::Deserialize, schemars::JsonSchema, PartialEq, Eq,
)]
#[serde(rename_all = "snake_case")]
pub enum LabelDisplay {
    Raw,
    UnderscoresToSpaces,
    TitleCase,
}
impl LabelDisplay {
    pub fn apply(self, value: &str) -> String {
        match self {
            Self::Raw => value.into(),
            Self::UnderscoresToSpaces => value.replace('_', " "),
            Self::TitleCase => transformer_title(value),
        }
    }
}

/// Presentation casing for transformer names. Underscores in an engineering
/// identity become word spaces on paper; the identity itself stays unchanged.
/// A cased run starts with uppercase and continues lowercase; digits and
/// apostrophes start a new run. Other separators and whitespace are preserved.
pub fn transformer_title(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    let mut previous_cased = false;
    let mut word_separator = false;
    for ch in name.chars() {
        if ch == '_' {
            word_separator = true;
            previous_cased = false;
            continue;
        }
        if word_separator {
            if !out.ends_with(' ') && !ch.is_whitespace() && !out.is_empty() {
                out.push(' ');
            }
            word_separator = false;
        }
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
            ("upgrade_munyinya", "Upgrade Munyinya"),
            (
                "Fill_In_Bukorota TRANSFORMER",
                "Fill In Bukorota Transformer",
            ),
            ("Fill__In_", "Fill In"),
            ("CYIMPUNGA-TR02a", "Cyimpunga-Tr02A"),
            ("ruko's site", "Ruko'S Site"),
            ("  éCOLE  nord", "  École  Nord"),
        ] {
            assert_eq!(transformer_title(raw), shown);
        }
        let mut text = BTreeMap::from([
            ("transformer".into(), "Fill_In_Bukorota".into()),
            ("title".into(), "Fill_In_Bukorota".into()),
            ("drawing".into(), "LV-2026-017".into()),
            ("project".into(), "czgmdwth_gisagara".into()),
        ]);
        plan_text(&mut text);
        assert_eq!(text["transformer"], "Fill In Bukorota");
        assert_eq!(text["title"], "Fill In Bukorota");
        assert_eq!(text["drawing"], "LV-2026-017");
        assert_eq!(text["project"], "czgmdwth_gisagara");
    }
}
