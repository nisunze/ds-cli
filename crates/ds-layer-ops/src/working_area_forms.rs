//! Which survey forms the working area loads — the three operations
//! `ds survey working-area forms|select|clear` run natively and the Server's
//! route runs for the project a caller names. ONE owner, no rule of its own.
//!
//! The decision is the kernel's (`ds_command_kernel::survey_working_area_forms`):
//! this module reads the form catalogue from the project's layer document
//! (the same source the drawer reads, so no new backend read), reads the
//! selection this host remembers through `ds_layer_store::working_area_forms`
//! (a sibling of the drawer's visibility file, same lane/uid/project key),
//! asks, and persists exactly the `next_selection` the kernel returned, under
//! the store's lock so two concurrent selections never replace each other.

use ds_cli_contract::outcome::Failure;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{DocumentRead, LayerDocuments, Preferences, Scope};

pub const FORM_REMEDY: &str =
    "copy slugs from `ds survey working-area forms --output json` (.data.forms[].slug)";
pub const NAME_FORMS_REMEDY: &str = "pass --form <slug> (repeatable), --all, or --none";
pub const MAX_FORMS: usize = ds_command_kernel::survey_working_area_forms::MAX_FORMS;

/// What a selection replaces the choice with: exactly these forms, every
/// form, or no form. Exactly one is set; the CLI and the Server route both
/// build this shape, so the wire body is the request type.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SelectRequest {
    pub forms: Vec<String>,
    pub all: bool,
    pub none: bool,
}

impl SelectRequest {
    fn op(&self) -> Result<Value, Failure> {
        let named = !self.forms.is_empty();
        match (named, self.all, self.none) {
            (true, false, false) => Ok(json!({"kind": "select", "forms": self.forms})),
            (false, true, false) => Ok(json!({"kind": "all"})),
            (false, false, true) => Ok(json!({"kind": "none"})),
            (false, false, false) => Err(Failure::invalid(
                "no_forms_named",
                "name at least one form to select, or select all or none",
            )
            .remedy(NAME_FORMS_REMEDY)),
            _ => Err(Failure::invalid(
                "ambiguous_selection",
                "name forms, or all, or none — not more than one of them",
            )
            .remedy("pass --form <slug>… alone, --all alone, or --none alone")),
        }
    }
}

fn ask(read: &DocumentRead, selection: Option<&Vec<String>>, op: Value) -> Result<Value, Failure> {
    let request = json!({
        "schema": ds_command_kernel::survey_working_area_forms::SCHEMA,
        "project": read.scope.project,
        "document": read.document,
        "selection": selection,
        "op": op,
    });
    let bytes = serde_json::to_vec(&request).expect("working-area forms request encodes");
    let answer =
        ds_command_kernel::survey_working_area_forms::evaluate(&bytes).map_err(|message| {
            match message.split_once(": ") {
                Some(("project_context_changed", rest)) => {
                    Failure::conflict("project_context_changed", rest.to_owned())
                        .remedy("run the command again against the current selected project")
                }
                Some(("unknown_form", rest)) => {
                    Failure::invalid("unknown_form", rest.to_owned()).remedy(FORM_REMEDY)
                }
                Some(("no_forms_named", rest)) => {
                    Failure::invalid("no_forms_named", rest.to_owned()).remedy(NAME_FORMS_REMEDY)
                }
                _ => Failure::internal("working_area_forms_refused", message)
                    .remedy("update ds and report the working-area forms contract failure"),
            }
        })?;
    Ok(serde_json::from_str(&answer).expect("kernel answers JSON"))
}

fn stored(preferences: &Preferences, scope: &Scope) -> Result<Option<Vec<String>>, Failure> {
    ds_layer_store::working_area_forms::read_at(
        preferences.root(),
        &scope.lane,
        &scope.uid,
        &scope.project,
    )
    .map_err(|message| {
        Failure::invalid("local_layer_refused", message).remedy(crate::LOCAL_STORE_REMEDY)
    })
}

fn shaped(read: &DocumentRead, mut answer: Value, receipt: Option<&Value>) -> Value {
    answer["lane"] = json!(read.scope.lane);
    answer["selection_source"] = json!("native_local");
    if let Some(receipt) = receipt {
        answer["persisted"] = receipt["persisted"].clone();
        answer["revision"] = receipt["revision"].clone();
    }
    answer
}

/// The project's forms with whether each loads, what loads, and the remedy
/// when nothing does. A read persists nothing.
pub fn read(
    documents: &mut dyn LayerDocuments,
    preferences: &Preferences,
) -> Result<Value, Failure> {
    let read = documents.read(false)?;
    let selection = stored(preferences, &read.scope)?;
    let answer = ask(&read, selection.as_ref(), json!({"kind": "read"}))?;
    Ok(shaped(&read, answer, None))
}

/// Replace the selection. The kernel decides the next entry under the store's
/// lock; an unchanged choice writes nothing and answers `changed: false`.
pub fn select(
    documents: &mut dyn LayerDocuments,
    preferences: &Preferences,
    request: &SelectRequest,
) -> Result<Value, Failure> {
    let op = request.op()?;
    if request.forms.len() > MAX_FORMS {
        return Err(
            Failure::invalid("unknown_form", format!("name at most {MAX_FORMS} forms"))
                .remedy(FORM_REMEDY),
        );
    }
    transition(documents, preferences, op)
}

/// Forget the choice on this host: back to never chosen, which loads nothing.
pub fn clear(
    documents: &mut dyn LayerDocuments,
    preferences: &Preferences,
) -> Result<Value, Failure> {
    transition(documents, preferences, json!({"kind": "clear"}))
}

fn transition(
    documents: &mut dyn LayerDocuments,
    preferences: &Preferences,
    op: Value,
) -> Result<Value, Failure> {
    let read = documents.read(false)?;
    let (receipt, answer) = ds_layer_store::working_area_forms::update_at(
        preferences.root(),
        &read.scope.lane,
        &read.scope.uid,
        &read.scope.project,
        |current| {
            documents.check_scope(&read.scope)?;
            let answer = ask(&read, current, op)?;
            let next: Option<Vec<String>> =
                serde_json::from_value(answer["next_selection"].clone())
                    .expect("kernel next_selection is a list or null");
            Ok((next, answer))
        },
    )
    .map_err(|error| match error {
        ds_layer_store::working_area_forms::UpdateError::Store(message) => {
            Failure::invalid("local_layer_refused", message).remedy(crate::LOCAL_STORE_REMEDY)
        }
        ds_layer_store::working_area_forms::UpdateError::Transition(failure) => failure,
    })?;
    Ok(shaped(&read, answer, Some(&receipt)))
}

pub fn render(data: &Value) -> String {
    let mut out = format!(
        "project {} · {} of {} survey forms load in the working area{}\n",
        data["project"].as_str().unwrap_or("?"),
        data["selected_count"],
        data["form_count"],
        match data["changed"].as_bool() {
            Some(true) => " · saved locally",
            Some(false) => " · unchanged",
            None => "",
        }
    );
    for row in data["forms"].as_array().into_iter().flatten() {
        out.push_str(&format!(
            "{} {:<32} {}{}\n",
            if row["selected"].as_bool().unwrap_or(false) {
                "[x]"
            } else {
                "[ ]"
            },
            row["slug"].as_str().unwrap_or("?"),
            row["label"].as_str().unwrap_or("?"),
            row["count"]
                .as_u64()
                .map(|count| format!(" · {count} rows held"))
                .unwrap_or_default(),
        ));
    }
    if let Some(stale) = data["stale"].as_array().filter(|stale| !stale.is_empty()) {
        out.push_str(&format!(
            "stale (chosen, no longer a form here): {}\n",
            stale
                .iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    if let Some(remedy) = data["remedy"].as_str() {
        out.push_str(&format!("{remedy}\n"));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::Fixture;

    fn fixture(project: &str) -> Fixture {
        let mut fixture = Fixture::new("u1", project);
        fixture.document["survey_layers"] = json!([
            {"key": "tr", "label": "Transformers", "layer_ids": ["ds-tr"], "style_ref": "tr"},
            {"key": "lv", "label": "LV poles", "layer_ids": ["ds-lv"], "style_ref": "lv"},
            {"key": "mv", "label": "MV poles", "layer_ids": ["ds-mv"], "style_ref": "mv"},
        ]);
        fixture
    }

    fn named(forms: &[&str]) -> SelectRequest {
        SelectRequest {
            forms: forms.iter().map(|f| (*f).to_owned()).collect(),
            ..SelectRequest::default()
        }
    }

    #[test]
    fn never_chosen_loads_nothing_then_a_choice_persists_and_clears() {
        let tmp = tempfile::tempdir().unwrap();
        let preferences = Preferences::at(tmp.path().to_owned());
        let mut documents = fixture("p1");

        let first = read(&mut documents, &preferences).unwrap();
        assert_eq!(first["chosen"], false);
        assert_eq!(first["loads"], json!([]));
        assert_eq!(first["form_count"], 3);
        assert!(
            first["remedy"]
                .as_str()
                .unwrap()
                .contains("working-area select")
        );
        assert_eq!(first["selection_source"], "native_local");
        assert!(!tmp.path().join("working-area-forms.json").exists());

        let chosen = select(&mut documents, &preferences, &named(&["mv", "tr"])).unwrap();
        assert_eq!(chosen["changed"], true);
        assert_eq!(chosen["loads"], json!(["tr", "mv"]));
        assert_eq!(chosen["persisted"], "native_local");
        assert_eq!(chosen["revision"], 1);

        let again = select(&mut documents, &preferences, &named(&["tr", "mv"])).unwrap();
        assert_eq!(again["changed"], false);
        assert_eq!(
            again["revision"], 1,
            "an identical choice is not a revision"
        );

        let read_back = read(&mut documents, &preferences).unwrap();
        assert_eq!(read_back["chosen"], true);
        assert_eq!(read_back["loads"], json!(["tr", "mv"]));
        assert_eq!(read_back["remedy"], Value::Null);

        let mut other_project = fixture("p2");
        assert_eq!(
            read(&mut other_project, &preferences).unwrap()["chosen"],
            false,
            "another project has its own choice"
        );

        let cleared = clear(&mut documents, &preferences).unwrap();
        assert_eq!(cleared["changed"], true);
        assert_eq!(cleared["chosen"], false);
        assert_eq!(read(&mut documents, &preferences).unwrap()["chosen"], false);
    }

    #[test]
    fn all_and_none_are_choices_and_unknown_forms_are_refused_by_name() {
        let tmp = tempfile::tempdir().unwrap();
        let preferences = Preferences::at(tmp.path().to_owned());
        let mut documents = fixture("p1");
        let all = select(
            &mut documents,
            &preferences,
            &SelectRequest {
                all: true,
                ..SelectRequest::default()
            },
        )
        .unwrap();
        assert_eq!(all["loads"], json!(["tr", "lv", "mv"]));
        let none = select(
            &mut documents,
            &preferences,
            &SelectRequest {
                none: true,
                ..SelectRequest::default()
            },
        )
        .unwrap();
        assert_eq!(none["chosen"], true);
        assert_eq!(none["loads"], json!([]));
        assert!(none["remedy"].as_str().is_some());

        let unknown = select(&mut documents, &preferences, &named(&["tr", "nope"])).unwrap_err();
        assert_eq!(unknown.code(), "unknown_form");
        assert!(unknown.message().contains("nope"));
        assert!(!unknown.message().contains("tr,"));
        assert_eq!(
            read(&mut documents, &preferences).unwrap()["loads"],
            json!([]),
            "a refused selection changes nothing"
        );
        assert_eq!(
            select(&mut documents, &preferences, &SelectRequest::default())
                .unwrap_err()
                .code(),
            "no_forms_named"
        );
        assert_eq!(
            select(
                &mut documents,
                &preferences,
                &SelectRequest {
                    forms: vec!["tr".into()],
                    all: true,
                    none: false
                }
            )
            .unwrap_err()
            .code(),
            "ambiguous_selection"
        );
    }

    #[test]
    fn a_scope_that_moves_between_read_and_write_is_refused_before_the_effect() {
        let tmp = tempfile::tempdir().unwrap();
        let preferences = Preferences::at(tmp.path().to_owned());
        let mut documents = fixture("p1");
        documents.scope_after_read = Some(Scope {
            lane: "canary".into(),
            uid: "u2".into(),
            project: "p1".into(),
        });
        let refused = select(&mut documents, &preferences, &named(&["tr"])).unwrap_err();
        assert_eq!(refused.code(), "project_context_changed");
        assert!(!tmp.path().join("working-area-forms.json").exists());
    }

    #[test]
    fn the_text_rendering_names_the_choice_and_the_remedy() {
        let tmp = tempfile::tempdir().unwrap();
        let preferences = Preferences::at(tmp.path().to_owned());
        let mut documents = fixture("p1");
        let text = render(&read(&mut documents, &preferences).unwrap());
        assert!(text.starts_with("project p1 · 0 of 3 survey forms load"));
        assert!(text.contains("[ ] tr"));
        assert!(text.contains("working-area select"));
        let text = render(&select(&mut documents, &preferences, &named(&["lv"])).unwrap());
        assert!(text.contains("1 of 3 survey forms load in the working area · saved locally"));
        assert!(text.contains("[x] lv"));
    }
}
