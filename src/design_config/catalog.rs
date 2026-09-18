//! What a project's customer and meter vocabularies will do to a report.
//!
//! Reporting validates every grouped description against the catalogue's
//! canonical names and, when a description is not among them, rewrites it to a
//! fallback instead of leaving it out. Three properties of a catalogue decide
//! that rewrite, and none of them is visible from the rows themselves:
//!
//! * which row supplies the fallback — a `default_category` project setting
//!   when one is stored, otherwise whichever named row happens to be first;
//! * which source labels are claimed by two canonical names at once, because
//!   an alias owned twice is dropped from the alias map and its customers then
//!   land in the fallback;
//! * which rows carry no canonical name at all, because they are inert to
//!   reporting but still occupy the catalogue a person reads.
//!
//! This reads them off the configuration document the client already fetched.
//! It decides nothing and changes nothing; it is the sentence a caller needs
//! before it retires, renames or rebinds anything.
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;
use std::collections::BTreeSet;

/// At most this many ambiguous aliases or catalogue scopes are listed; the
/// rest are counted. A vocabulary answer that scrolls is a vocabulary answer
/// nobody reads.
const LISTED: usize = 20;

/// The customer and meter vocabulary hazards of one configuration document.
/// `sheets` is the document's `sheets` object, as the configuration client
/// returns it.
pub fn catalog_hazards(sheets: &Value) -> Value {
    json!({
        "customer": hazards(
            &sheets["cust_category"],
            stored_default(&sheets["project_settings"], "default_category"),
            true,
        ),
        "meter": hazards(&sheets["cust_meter_type"], None, false),
    })
}

fn hazards(rows: &Value, stored: Option<String>, scoped: bool) -> Value {
    let rows = rows.as_array().map(Vec::as_slice).unwrap_or_default();
    let mut blank = 0usize;
    let mut owners: BTreeMap<String, (String, BTreeSet<String>)> = BTreeMap::new();
    let mut scopes: BTreeMap<(String, String), usize> = BTreeMap::new();
    let mut first: Option<String> = None;
    for row in rows {
        let Some(clean) = canonical(row) else {
            blank += 1;
            continue;
        };
        if first.is_none() {
            first = Some(clean.clone());
        }
        if scoped {
            let key = (text(&row["client"]), text(&row["country"]));
            *scopes.entry(key).or_default() += 1;
        }
        for name in super::alias_list(&row["misspelled_names"])
            .into_iter()
            .chain(std::iter::once(clean.clone()))
        {
            let entry = owners
                .entry(name.trim().to_lowercase())
                .or_insert_with(|| (name.trim().to_owned(), BTreeSet::new()));
            entry.1.insert(clean.clone());
        }
    }

    let ambiguous: Vec<&(String, BTreeSet<String>)> = owners
        .values()
        .filter(|(_, categories)| categories.len() > 1)
        .collect();
    let listed: Vec<Value> = ambiguous
        .iter()
        .take(LISTED)
        .map(|(label, categories)| {
            json!({"alias": label, "categories": categories.iter().collect::<Vec<_>>()})
        })
        .collect();

    // The fallback sentence names both the value and how it was decided,
    // because "first row" is an accident a caller is entitled to replace and
    // "project setting" is a decision it should not silently overwrite.
    let (fallback, decided_by) = match stored.filter(|value| !value.trim().is_empty()) {
        Some(value) => (Some(value), "project_setting"),
        None => (first, "catalog_order"),
    };

    let mut answer = Map::new();
    answer.insert("rows".into(), json!(rows.len()));
    answer.insert("unnamed_rows".into(), json!(blank));
    answer.insert("ambiguous_aliases".into(), Value::Array(listed));
    answer.insert(
        "ambiguous_aliases_omitted".into(),
        json!(ambiguous.len().saturating_sub(LISTED)),
    );
    answer.insert(
        "fallback".into(),
        match fallback {
            Some(name) => json!({"category": name, "decided_by": decided_by}),
            None => json!({"category": Value::Null, "decided_by": "none"}),
        },
    );
    if scoped {
        let listed: Vec<Value> = scopes
            .iter()
            .take(LISTED)
            .map(|((client, country), count)| {
                json!({"client": client, "country": country, "rows": count})
            })
            .collect();
        answer.insert(
            "scopes_omitted".into(),
            json!(scopes.len().saturating_sub(listed.len())),
        );
        answer.insert("scopes".into(), Value::Array(listed));
    }
    Value::Object(answer)
}

/// One catalogue row's canonical name, or `None` when the row declares none.
/// A row with no canonical name groups nothing and validates nothing.
pub fn canonical(row: &Value) -> Option<String> {
    let name = row["clean_name"].as_str()?.trim();
    (!name.is_empty()).then(|| name.to_owned())
}

fn text(value: &Value) -> String {
    value.as_str().unwrap_or_default().trim().to_owned()
}

fn stored_default(settings: &Value, parameter: &str) -> Option<String> {
    let wanted = super::canonical_parameter_name(parameter);
    settings
        .as_array()?
        .iter()
        .find(|row| {
            row["parameter"]
                .as_str()
                .is_some_and(|name| super::canonical_parameter_name(name) == wanted)
        })
        .and_then(|row| row["value"].as_str())
        .map(|value| value.trim().to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn document() -> Value {
        json!({
            "cust_category": [
                {"clean_name":"Health Post","client":"EDCL","country":"Rwanda",
                 "misspelled_names":["Health Post","Centre de Sante"]},
                {"clean_name":"School","client":"EDCL","country":"Rwanda",
                 "misspelled_names":["School","Ecole Primaire"]},
                {"clean_name":null,"client":"EDCL","country":"Rwanda","misspelled_names":[]},
                {"clean_name":"Centre de Sante","client":"ADERM","country":"Tchad",
                 "misspelled_names":"Centre de Sante ## Clinique"},
                {"clean_name":"Education_I","client":"ADERM","country":"Tchad",
                 "misspelled_names":["Education_I","Ecole Primaire"]}
            ],
            "cust_meter_type": [
                {"clean_name":"Single Phase","misspelled_names":["Residential"]},
                {"clean_name":"Three Phase","misspelled_names":["3"]}
            ],
            "project_settings": []
        })
    }

    #[test]
    fn a_label_two_categories_claim_is_named_with_both_owners() {
        let hazards = catalog_hazards(&document());
        let ambiguous = hazards["customer"]["ambiguous_aliases"]
            .as_array()
            .expect("aliases");
        let pairs: Vec<(&str, Vec<&str>)> = ambiguous
            .iter()
            .map(|entry| {
                (
                    entry["alias"].as_str().expect("alias"),
                    entry["categories"]
                        .as_array()
                        .expect("categories")
                        .iter()
                        .map(|v| v.as_str().expect("name"))
                        .collect(),
                )
            })
            .collect();
        assert_eq!(
            pairs,
            vec![
                ("Centre de Sante", vec!["Centre de Sante", "Health Post"]),
                ("Ecole Primaire", vec!["Education_I", "School"]),
            ],
            "both contested labels, with every canonical name claiming them"
        );
        assert_eq!(hazards["customer"]["ambiguous_aliases_omitted"], json!(0));
    }

    #[test]
    fn an_unnamed_row_is_counted_and_never_becomes_the_fallback() {
        let hazards = catalog_hazards(&document());
        assert_eq!(hazards["customer"]["unnamed_rows"], json!(1));
        assert_eq!(hazards["customer"]["rows"], json!(5));
        assert_eq!(
            hazards["customer"]["fallback"],
            json!({"category":"Health Post","decided_by":"catalog_order"})
        );
    }

    #[test]
    fn a_stored_default_category_replaces_the_first_row_as_the_fallback() {
        let mut document = document();
        document["project_settings"] =
            json!([{"parameter":"Default-Category","value":"Residential"}]);
        let hazards = catalog_hazards(&document);
        assert_eq!(
            hazards["customer"]["fallback"],
            json!({"category":"Residential","decided_by":"project_setting"})
        );
    }

    #[test]
    fn meter_types_have_no_governed_default_so_the_first_row_decides() {
        let hazards = catalog_hazards(&document());
        assert_eq!(
            hazards["meter"]["fallback"],
            json!({"category":"Single Phase","decided_by":"catalog_order"})
        );
        assert_eq!(
            hazards["meter"].get("scopes"),
            None,
            "meter rows carry no client or country"
        );
    }

    #[test]
    fn a_catalogue_merged_from_two_clients_reports_both_scopes() {
        let hazards = catalog_hazards(&document());
        assert_eq!(
            hazards["customer"]["scopes"],
            json!([
                {"client":"ADERM","country":"Tchad","rows":2},
                {"client":"EDCL","country":"Rwanda","rows":2}
            ])
        );
    }

    #[test]
    fn two_categories_of_the_same_client_contest_a_label_just_as_visibly() {
        // Read off project czgmdwth_gisagara on 2026-09-18: `Chateau d'eau 1`
        // is listed by both Administration_I and Pompage_Eau_I, and neither is
        // the Rwandan half of that merged catalogue. A contested label is a
        // property of the alias map, not of the country columns, so nothing
        // here may key on client or country.
        let document = json!({"cust_category":[
            {"clean_name":"Administration_I","client":"ADERM","country":"Tchad",
             "misspelled_names":["Delegation","Chateau d'eau 1"]},
            {"clean_name":"Pompage_Eau_I","client":"ADERM","country":"Tchad",
             "misspelled_names":["Point d'eau","Chateau d'eau 1"]}
        ]});
        assert_eq!(
            catalog_hazards(&document)["customer"]["ambiguous_aliases"],
            json!([{"alias":"Chateau d'eau 1",
                    "categories":["Administration_I","Pompage_Eau_I"]}])
        );
    }

    #[test]
    fn an_empty_catalogue_names_no_fallback_instead_of_inventing_one() {
        let hazards = catalog_hazards(&json!({"cust_category":[],"cust_meter_type":[]}));
        assert_eq!(
            hazards["customer"]["fallback"],
            json!({"category":Value::Null,"decided_by":"none"})
        );
        assert_eq!(hazards["meter"]["rows"], json!(0));
    }
}
