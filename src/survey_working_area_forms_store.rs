//! The same working-area choice on the shared preference store. A browser
//! supplies held catalogue facts and closed command arguments, never keys,
//! selection policy, persistence instructions or refusal classification.
use crate::durable_store::{Call, Request};
use crate::gateway_exchange::retained::Edge;
use crate::ui_preferences::{Key, Operation};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::BTreeMap;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Invocation {
    command: String,
    #[serde(default)]
    args: Args,
    context: Context,
    held: Held,
}
#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct Args {
    #[serde(default)]
    form: Vec<String>,
    #[serde(default)]
    all: bool,
    #[serde(default)]
    none: bool,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Context {
    uid: String,
    project: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Held {
    document: Value,
    #[serde(default)]
    counts: BTreeMap<String, u64>,
}

fn refused(command: &str, code: &str, class: &str, message: &str, remedy: &str) -> Value {
    json!({"v":1,"command":command,"contract":1,"status":"error","error":{
        "class":class,"code":code,"message":message,"retryable":false,"remedy":remedy}})
}
fn error(command: &str, text: &str) -> Value {
    let (code, message) = text
        .split_once(": ")
        .unwrap_or(("working_area_forms_refused", text));
    let (class, remedy) = match code {
        "project_context_changed" => (
            "conflict",
            "run the command again against the current selected project",
        ),
        "unknown_form" => (
            "invalid_input",
            "copy slugs from `ds survey working-area forms --output json` (.data.forms[].slug)",
        ),
        "no_forms_named" | "ambiguous_selection" => (
            "invalid_input",
            "pass --form <slug> (repeatable), --all, or --none, alone",
        ),
        "auth_context_mismatch" => (
            "unauthorized",
            "sign in to the current account and retry the unchanged choice",
        ),
        "durable_store_owner_changed" => {
            ("conflict", "retry against the current signed-in account")
        }
        _ => (
            "unavailable",
            "restore the local Kernel store and retry the unchanged choice",
        ),
    };
    refused(command, code, class, message, remedy)
}
fn op(invocation: &Invocation) -> Result<Value, String> {
    match invocation.command.as_str() {
        "survey.working-area.forms" => Ok(json!({"kind":"read"})),
        "survey.working-area.clear" => Ok(json!({"kind":"clear"})),
        "survey.working-area.select" => {
            let args = &invocation.args;
            let named = !args.form.is_empty();
            if usize::from(named) + usize::from(args.all) + usize::from(args.none) > 1 {
                return Err(
                    "ambiguous_selection: name forms, or all, or none — not more than one of them"
                        .into(),
                );
            }
            if named {
                Ok(json!({"kind":"select","forms":args.form}))
            } else if args.all {
                Ok(json!({"kind":"all"}))
            } else if args.none {
                Ok(json!({"kind":"none"}))
            } else {
                Err(
                    "no_forms_named: name at least one form to select, or select all or none"
                        .into(),
                )
            }
        }
        _ => Err("working_area_forms_refused: unknown working-area forms command".into()),
    }
}

async fn apply(edge: &impl Edge, principal: &str, invocation: Invocation) -> Result<Value, String> {
    if principal.is_empty() || invocation.context.uid != principal {
        return Err(
            "auth_context_mismatch: the selection belongs to the current signed-in account".into(),
        );
    }
    let operation = op(&invocation)?;
    let project = invocation.context.project;
    let mut input = json!({"schema":super::SCHEMA,"project":project,"document":invocation.held.document,
        "counts":invocation.held.counts,"selection":null,"op":operation});
    // Reject invalid catalogue/arguments before spending a storage read.
    super::evaluate(&serde_json::to_vec(&input).map_err(|error| error.to_string())?)?;
    let observed = edge
        .store(Call {
            request: Request::UiPreferences {
                operation: Operation::Get {
                    project: Some(project.clone()),
                    keys: vec![Key::WorkingAreaForms],
                },
            },
            expect_owner: None,
        })
        .await
        .map_err(|error| format!("{}: {}", error.code, error.message))?;
    let owner = observed["owner"]
        .as_str()
        .ok_or("durable_store_owner_changed: the preference owner was not returned")?;
    input["selection"] = observed["values"]["working_area_forms"].clone();
    let mut data: Value = serde_json::from_str(&super::evaluate(
        &serde_json::to_vec(&input).map_err(|error| error.to_string())?,
    )?)
    .map_err(|error| error.to_string())?;
    if data["changed"] == true {
        let operation = if data["next_selection"].is_null() {
            Operation::Clear {
                project: Some(project),
                key: Key::WorkingAreaForms,
            }
        } else {
            Operation::Set {
                project: Some(project),
                key: Key::WorkingAreaForms,
                value: data["next_selection"].clone(),
                now: edge.now_ms(),
            }
        };
        edge.store(Call {
            request: Request::UiPreferences { operation },
            expect_owner: Some(owner.into()),
        })
        .await
        .map_err(|error| format!("{}: {}", error.code, error.message))?;
    }
    data["selection_source"] = json!("browser_local");
    Ok(data)
}

pub async fn invoke(edge: &impl Edge, principal: &str, value: Value) -> Value {
    let command = value["command"]
        .as_str()
        .unwrap_or("survey.working-area.forms")
        .to_owned();
    let invocation = match serde_json::from_value::<Invocation>(value) {
        Ok(invocation) => invocation,
        Err(cause) => return error(&command, &format!("working_area_forms_refused: {cause}")),
    };
    match apply(edge, principal, invocation).await {
        Ok(data) => json!({"v":1,"command":command,"contract":1,"status":"ok","data":data}),
        Err(cause) => error(&command, &cause),
    }
}

#[cfg(all(feature = "wasm", target_arch = "wasm32"))]
mod browser {
    use crate::durable_store::port::StoreHost;
    use crate::gateway_exchange::browser::{facts, typed, GatewayHost};
    use crate::gateway_exchange::retained::browser::BrowserEdge;
    use wasm_bindgen::prelude::*;

    #[wasm_bindgen(js_name = surveyWorkingAreaFormsInvoke)]
    pub async fn invoke(
        host: GatewayHost,
        store: StoreHost,
        invocation: JsValue,
    ) -> Result<JsValue, JsValue> {
        let value = tsify::serde_wasm_bindgen::from_value(invocation)
            .map_err(|cause| js_sys::Error::new(&cause.to_string()))?;
        let principal = facts(&host)?.principal;
        let edge = BrowserEdge::new(JsValue::clone(&host).unchecked_into(), store);
        let mut answer = super::invoke(&edge, &principal, value).await;
        if facts(&host)?.principal != principal {
            answer = super::error(
                answer["command"]
                    .as_str()
                    .unwrap_or("survey.working-area.forms"),
                "auth_context_mismatch: the account changed before the choice was adopted",
            );
        }
        typed(&answer)
    }
    use wasm_bindgen::JsCast;
}

#[cfg(test)]
#[path = "survey_working_area_forms_store_tests.rs"]
mod tests;
