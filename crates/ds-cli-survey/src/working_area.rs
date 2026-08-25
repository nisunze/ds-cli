//! `ds survey working-area read` — the Working Area the desktop has applied,
//! and what it has cached under it.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{Authority, Command, Effect, Example, Execution};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Value, json};

use crate::DESCRIPTOR_ARG;

pub mod read {
    use super::*;

    pub static COMMAND: Command = Command {
        id: "survey.working-area.read",
        path: &["survey", "working-area", "read"],
        contract: 1,
        summary: "The applied Working Area and per-form cached entry counts.",
        purpose: "\
Reports the survey scope the desktop currently has applied — full project, an \
admin unit, or a drawn boundary, with any date and surveyor filters — and how \
many entries each form has cached under it. To materialize the whole project \
run `ds map survey download --entire-project`; this command changes nothing.",
        effect: Effect::ReadOnly,
        authority: Authority::Project,
        execution: Execution::Sync,
        args: &[DESCRIPTOR_ARG],
        output: "\
`project`, `mode` (full|admin|draw), `applied`, `applied_at`, `full_project_load`, \
`date_from`, `date_to`, `surveyors`, `admin` (code, name, level), \
`boundary_vertices`, `forms`, `cached` per form and `cached_total`. No survey row.",
        examples: &[Example {
            command: "ds survey working-area read --output json",
            note: "cached_total of 0 with applied=false means nothing has been materialized yet.",
            runnable: false,
        }],
        refusals: &[
            crate::NOT_PAIRED,
            crate::AMBIGUOUS,
            crate::UNREACHABLE,
            crate::PAIRING_REJECTED,
            crate::SURVEY_REFUSED,
            crate::UNSUPPORTED,
            crate::UNREADABLE,
            crate::SIGNED_OUT,
        ],
        reference: Some("docs/reference/survey.md"),
        availability: crate::paired_availability,
    };

    pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
        let descriptor = crate::paired(inputs.value("desktop-descriptor"))?;
        crate::invoke(
            &descriptor,
            &crate::WORKING_AREA_READ,
            json!({}),
            crate::READ_TIMEOUT,
        )
        .map(receipt)
        .map_err(crate::classify_survey_failure)
    }

    fn receipt(result: Value) -> Value {
        json!({
            "project": result["project"],
            "mode": result["mode"],
            "applied": result["applied"],
            "applied_at": result["appliedAt"],
            "full_project_load": result["fullProjectLoad"],
            "date_from": result["dateFrom"],
            "date_to": result["dateTo"],
            "surveyors": result["surveyors"],
            "admin": result["admin"],
            "boundary_vertices": result["boundaryVertices"],
            "forms": result["forms"],
            "cached": result["cached"],
            "cached_total": result["cachedTotal"],
            "rows_returned": 0,
        })
    }

    pub fn render(data: &Value) -> String {
        let mut out = format!(
            "{}  ·  working area: {}{}  ·  {} cached across {}\n",
            data["project"].as_str().unwrap_or("?"),
            data["mode"].as_str().unwrap_or("?"),
            if data["applied"].as_bool() == Some(true) {
                " (applied)"
            } else {
                " (not applied)"
            },
            crate::plural(data["cached_total"].as_u64().unwrap_or(0), "entry"),
            crate::plural(data["forms"].as_u64().unwrap_or(0), "form"),
        );
        if let Some(admin) = data["admin"].as_object() {
            out.push_str(&format!(
                "  admin unit: {} {} ({})\n",
                admin["level"].as_str().unwrap_or(""),
                admin["name"].as_str().unwrap_or("?"),
                admin["code"].as_str().unwrap_or("?")
            ));
        }
        let from = data["date_from"].as_str().unwrap_or("");
        let to = data["date_to"].as_str().unwrap_or("");
        if !from.is_empty() || !to.is_empty() {
            out.push_str(&format!(
                "  dates: {} .. {}\n",
                if from.is_empty() { "-" } else { from },
                if to.is_empty() { "-" } else { to }
            ));
        }
        if let Some(cached) = data["cached"].as_object() {
            for (slug, count) in cached {
                out.push_str(&format!(
                    "  {:<32} {:>6}\n",
                    slug,
                    count.as_u64().unwrap_or(0)
                ));
            }
        }
        out
    }
}
