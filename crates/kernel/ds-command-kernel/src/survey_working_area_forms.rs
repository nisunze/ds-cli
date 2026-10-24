//! Which survey forms the working area loads — a per-host, per-project
//! operator choice, decided here and persisted by whichever host asked.
//!
//! # The defect this settles
//!
//! The map's working area used to load the survey entries of EVERY form the
//! project has (`loadAllSurveyData` walked `survey_layers` and queried each
//! one). A project with a dozen forms paid a dozen reads before the operator
//! had asked for any of them. The rule is now the operator's: the working area
//! loads exactly the forms they selected on this host for this project, and
//! **until they choose, it loads nothing**. There is no "all by default".
//!
//! # What is decided here
//!
//! For one project's form catalogue — the `survey_layers` of the assembled
//! layer document the Layers API already returns, so no new read is spent —
//! and the selection the host remembers for (lane, account, project):
//!
//! * **read** — every participating form with whether it is selected and, when
//!   the host already knows one, a cheap row count; the effective list the
//!   working area loads; whether the operator ever chose; a remedy when the
//!   answer is "nothing loads";
//! * **select** — replace the selection with an explicit list (unknown slugs
//!   are refused by name, duplicates fold, order is the catalogue's), with every
//!   form (`all`) or with no form (`none`); idempotent — an unchanged selection
//!   is `changed: false` and the host writes nothing;
//! * **clear** — forget the choice, back to "never chosen".
//!
//! Every answer carries the same projection, so a host renders one shape.
//!
//! # Hosts
//!
//! `ds` (natively and against a Server) reads the document through the native
//! client and the selection through the native layer store, under the same
//! (lane, uid, project) key the drawer's remembered visibility uses; the
//! browser holds both in its own stores and asks the vendored WASM. A
//! transition returns `next_selection`: `Some` is what to persist, `None` is
//! "remove the entry". Nothing here reads a clock, a file or a socket.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

pub const SCHEMA: &str = "ds.survey-working-area-forms/v1";
pub const MAX_INPUT_BYTES: usize = 32 * 1024 * 1024;
pub const MAX_FORMS: usize = 5_000;
pub const MAX_SLUG_CHARS: usize = 200;
pub const MAX_SELECTION_BYTES: usize = MAX_FORMS * (4 * MAX_SLUG_CHARS + 3) + 2;

#[cfg(feature = "durable-store")]
#[path = "survey_working_area_forms_store.rs"]
pub(crate) mod store;

/// The sentence a read carries when the working area would load nothing.
pub const NOTHING_SELECTED_REMEDY: &str = "no survey form is selected for the working area on this host, so it loads nothing; select forms with `ds survey working-area select --form <slug> …` or `--all`";
pub const NO_FORMS_REMEDY: &str = "this project has no active survey forms; enable forms with `ds survey project-forms plan/apply` before loading a working area";

// ── the request ─────────────────────────────────────────────────────────

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    schema: String,
    /// The project the caller believes it is asking about; the document must
    /// agree or the answer is `project_context_changed`.
    project: String,
    document: Document,
    /// The selection this host remembers for the project: `None` = the
    /// operator never chose here.
    #[serde(default)]
    selection: Option<Vec<String>>,
    /// Row counts the host already holds (a browser's cached entries); never
    /// fetched to answer this question.
    #[serde(default)]
    counts: BTreeMap<String, u64>,
    op: Op,
}

/// The assembled layer document reduced to what this question reads. Unknown
/// members are ignored so the raw `/layers` response parses as-is.
#[derive(Deserialize)]
struct Document {
    project_id: String,
    #[serde(default)]
    survey_layers: Vec<SurveyLayer>,
}

#[derive(Deserialize)]
struct SurveyLayer {
    key: String,
    #[serde(default)]
    label: Option<String>,
    #[serde(default)]
    layer_ids: Vec<String>,
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum Op {
    Read,
    /// Replace the selection with exactly these forms.
    Select {
        forms: Vec<String>,
    },
    /// Replace the selection with every form of the catalogue.
    All,
    /// Replace the selection with no form: the operator chose to load nothing.
    None,
    /// Forget the choice: the host removes its entry.
    Clear,
}

impl Op {
    fn name(&self) -> &'static str {
        match self {
            Op::Read => "read",
            Op::Select { .. } => "select",
            Op::All => "all",
            Op::None => "none",
            Op::Clear => "clear",
        }
    }
}

// ── the answer ──────────────────────────────────────────────────────────

#[derive(Serialize)]
struct FormRow<'a> {
    slug: &'a str,
    label: &'a str,
    selected: bool,
    layer_ids: &'a [String],
    /// A count the host already knew, or `null`: never a promise to fetch one.
    count: Option<u64>,
}

fn valid_slug(slug: &str) -> bool {
    !slug.is_empty()
        && slug.trim() == slug
        && slug.chars().count() <= MAX_SLUG_CHARS
        && !slug.chars().any(char::is_control)
}

/// Stored choices preserve authored spelling and ordering; catalogue admission
/// decides which still load. Invalid stored values are not rewritten on read.
pub(crate) fn admit_selection(value: &Value) -> Option<Value> {
    let forms = value.as_array().filter(|forms| forms.len() <= MAX_FORMS)?;
    let mut seen = BTreeSet::new();
    let mut admitted = Vec::new();
    for form in forms {
        let form = form.as_str().filter(|form| valid_slug(form))?;
        if seen.insert(form) { admitted.push(json!(form)); }
    }
    Some(Value::Array(admitted))
}

/// The operator's choice as the host will persist it, or as it read it.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Choice {
    Unset,
    Forms(Vec<String>),
}

impl Choice {
    fn from_stored(selection: Option<Vec<String>>) -> Self {
        match selection {
            Some(forms) => Choice::Forms(forms),
            None => Choice::Unset,
        }
    }
}

pub fn evaluate(input: &[u8]) -> Result<String, String> {
    if input.len() > MAX_INPUT_BYTES {
        return Err("survey working-area forms request exceeds its bound".into());
    }
    let request: Request = serde_json::from_slice(input)
        .map_err(|error| format!("invalid survey working-area forms request: {error}"))?;
    if request.schema != SCHEMA {
        return Err(format!(
            "unsupported survey working-area forms schema; expected {SCHEMA}"
        ));
    }
    if request.project.is_empty() || !valid_slug(&request.project) {
        return Err("this question needs the expected project".into());
    }
    if request.document.project_id != request.project {
        return Err(format!(
            "project_context_changed: the layer document belongs to {}, not {}",
            request.document.project_id, request.project
        ));
    }
    if request.document.survey_layers.len() > MAX_FORMS {
        return Err("the form catalogue exceeds its bound".into());
    }
    // The catalogue: one row per form, in document order, keys unique.
    let mut catalogue: Vec<&SurveyLayer> = Vec::new();
    let mut seen = BTreeSet::new();
    for layer in &request.document.survey_layers {
        if !valid_slug(&layer.key) {
            return Err(format!(
                "the layer document names an unusable survey form key {:?}",
                layer.key
            ));
        }
        if seen.insert(layer.key.as_str()) {
            catalogue.push(layer);
        }
    }
    let known: BTreeSet<&str> = seen;

    let before = Choice::from_stored(request.selection.clone());
    let after = match &request.op {
        Op::Read => before.clone(),
        Op::Select { forms } => {
            if forms.is_empty() {
                return Err(
                    "no_forms_named: name at least one form to select, or select all or none"
                        .into(),
                );
            }
            if forms.len() > MAX_FORMS {
                return Err("the selection exceeds its bound".into());
            }
            let mut unknown: Vec<&str> = Vec::new();
            let mut wanted: BTreeSet<&str> = BTreeSet::new();
            for form in forms {
                let form = form.trim();
                if !valid_slug(form) || !known.contains(form) {
                    if !unknown.contains(&form) {
                        unknown.push(form);
                    }
                    continue;
                }
                wanted.insert(form);
            }
            if !unknown.is_empty() {
                return Err(format!(
                    "unknown_form: not survey forms of project {}: {}",
                    request.project,
                    unknown
                        .iter()
                        .map(|form| if form.is_empty() { "\"\"" } else { form })
                        .collect::<Vec<_>>()
                        .join(", ")
                ));
            }
            Choice::Forms(
                catalogue
                    .iter()
                    .filter(|layer| wanted.contains(layer.key.as_str()))
                    .map(|layer| layer.key.clone())
                    .collect(),
            )
        }
        Op::All => Choice::Forms(catalogue.iter().map(|layer| layer.key.clone()).collect()),
        Op::None => Choice::Forms(Vec::new()),
        Op::Clear => Choice::Unset,
    };

    // The effective selection: chosen slugs that are still forms of the
    // project, in catalogue order. A chosen slug the project no longer has is
    // reported, never loaded.
    let (chosen, stored): (bool, BTreeSet<&str>) = match &after {
        Choice::Unset => (false, BTreeSet::new()),
        Choice::Forms(forms) => (true, forms.iter().map(String::as_str).collect()),
    };
    let stale: Vec<&str> = match &after {
        Choice::Forms(forms) => {
            let mut stale: Vec<&str> = Vec::new();
            for form in forms {
                if !known.contains(form.as_str()) && !stale.contains(&form.as_str()) {
                    stale.push(form);
                }
            }
            stale
        }
        Choice::Unset => Vec::new(),
    };
    let rows: Vec<FormRow> = catalogue
        .iter()
        .map(|layer| FormRow {
            slug: &layer.key,
            label: layer.label.as_deref().unwrap_or(&layer.key),
            selected: stored.contains(layer.key.as_str()),
            layer_ids: &layer.layer_ids,
            count: request.counts.get(&layer.key).copied(),
        })
        .collect();
    let loads: Vec<&str> = rows
        .iter()
        .filter(|row| row.selected)
        .map(|row| row.slug)
        .collect();
    let remedy = if catalogue.is_empty() {
        Some(NO_FORMS_REMEDY)
    } else if loads.is_empty() {
        Some(NOTHING_SELECTED_REMEDY)
    } else {
        None
    };

    let mut answer = json!({
        "schema": SCHEMA,
        "project": request.project,
        "op": request.op.name(),
        "chosen": chosen,
        "form_count": rows.len(),
        "selected_count": loads.len(),
        "loads": loads,
        "stale": stale,
        "forms": rows,
        "remedy": remedy,
    });
    if !matches!(request.op, Op::Read) {
        let changed = before != after;
        answer["changed"] = json!(changed);
        answer["next_selection"] = match &after {
            Choice::Unset => Value::Null,
            Choice::Forms(forms) => json!(forms),
        };
    }
    serde_json::to_string(&answer).map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn document(keys: &[&str]) -> Value {
        json!({
            "project_id": "p1",
            "survey_layers": keys.iter().map(|key| json!({
                "key": key,
                "label": format!("Form {key}"),
                "layer_ids": [format!("{key}_pts"), format!("{key}_pts__label")],
                "fields": [{"name": "x", "type": "string"}],
            })).collect::<Vec<_>>(),
            "layers": [],
            "sources": {},
        })
    }

    fn ask(selection: Option<Vec<&str>>, op: Value) -> Result<Value, String> {
        let request = json!({
            "schema": SCHEMA,
            "project": "p1",
            "document": document(&["tr", "lv", "mv"]),
            "selection": selection,
            "counts": {"tr": 12},
            "op": op,
        });
        evaluate(&serde_json::to_vec(&request).unwrap())
            .map(|answer| serde_json::from_str(&answer).unwrap())
    }

    #[test]
    fn never_chosen_loads_nothing_and_says_how_to_choose() {
        let answer = ask(None, json!({"kind": "read"})).unwrap();
        assert_eq!(answer["chosen"], false);
        assert_eq!(answer["form_count"], 3);
        assert_eq!(answer["selected_count"], 0);
        assert_eq!(answer["loads"], json!([]));
        assert_eq!(answer["remedy"], NOTHING_SELECTED_REMEDY);
        assert_eq!(answer["forms"][0]["count"], 12);
        assert_eq!(answer["forms"][1]["count"], Value::Null);
        assert!(
            answer.get("next_selection").is_none(),
            "a read persists nothing"
        );
    }

    #[test]
    fn select_replaces_in_catalogue_order_and_is_idempotent() {
        let answer = ask(None, json!({"kind": "select", "forms": ["mv", "tr", "tr"]})).unwrap();
        assert_eq!(answer["changed"], true);
        assert_eq!(answer["next_selection"], json!(["tr", "mv"]));
        assert_eq!(answer["loads"], json!(["tr", "mv"]));
        assert_eq!(answer["remedy"], Value::Null);
        let again = ask(
            Some(vec!["tr", "mv"]),
            json!({"kind": "select", "forms": ["mv", "tr"]}),
        )
        .unwrap();
        assert_eq!(again["changed"], false);
    }

    #[test]
    fn unknown_forms_are_refused_by_name() {
        let error = ask(
            None,
            json!({"kind": "select", "forms": ["tr", "nope", " "]}),
        )
        .unwrap_err();
        assert!(error.starts_with("unknown_form: "), "{error}");
        assert!(error.contains("nope"), "{error}");
        assert!(!error.contains("tr,"), "a known form is not named: {error}");
        assert_eq!(
            ask(None, json!({"kind": "select", "forms": []})).unwrap_err(),
            "no_forms_named: name at least one form to select, or select all or none"
        );
    }

    #[test]
    fn all_none_and_clear_are_distinct_choices() {
        let all = ask(None, json!({"kind": "all"})).unwrap();
        assert_eq!(all["next_selection"], json!(["tr", "lv", "mv"]));
        assert_eq!(all["selected_count"], 3);
        let none = ask(Some(vec!["tr"]), json!({"kind": "none"})).unwrap();
        assert_eq!(none["chosen"], true);
        assert_eq!(none["next_selection"], json!([]));
        assert_eq!(none["remedy"], NOTHING_SELECTED_REMEDY);
        let clear = ask(Some(vec!["tr"]), json!({"kind": "clear"})).unwrap();
        assert_eq!(clear["chosen"], false);
        assert_eq!(clear["changed"], true);
        assert_eq!(clear["next_selection"], Value::Null);
        let clear_again = ask(None, json!({"kind": "clear"})).unwrap();
        assert_eq!(clear_again["changed"], false);
    }

    #[test]
    fn a_chosen_form_the_project_no_longer_has_is_reported_not_loaded() {
        let answer = ask(Some(vec!["tr", "gone"]), json!({"kind": "read"})).unwrap();
        assert_eq!(answer["loads"], json!(["tr"]));
        assert_eq!(answer["stale"], json!(["gone"]));
    }

    #[test]
    fn the_document_must_be_about_the_named_project() {
        let request = json!({
            "schema": SCHEMA,
            "project": "p2",
            "document": document(&["tr"]),
            "op": {"kind": "read"},
        });
        let error = evaluate(&serde_json::to_vec(&request).unwrap()).unwrap_err();
        assert!(error.starts_with("project_context_changed: "), "{error}");
    }

    #[test]
    fn an_empty_catalogue_names_its_own_remedy() {
        let request = json!({
            "schema": SCHEMA,
            "project": "p1",
            "document": {"project_id": "p1"},
            "op": {"kind": "all"},
        });
        let answer: Value =
            serde_json::from_str(&evaluate(&serde_json::to_vec(&request).unwrap()).unwrap())
                .unwrap();
        assert_eq!(answer["form_count"], 0);
        assert_eq!(answer["remedy"], NO_FORMS_REMEDY);
        assert_eq!(answer["next_selection"], json!([]));
    }
}
