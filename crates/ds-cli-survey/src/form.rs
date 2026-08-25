//! `ds survey form list` and `ds survey form read` — the forms of the active
//! project, and one form's schema.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{Authority, Command, Effect, Example, Execution};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Value, json};

use crate::{DESCRIPTOR_ARG, FORM_ARG};

pub mod list {
    use super::*;

    pub static COMMAND: Command = Command {
        id: "survey.form.list",
        path: &["survey", "form", "list"],
        contract: 1,
        summary: "The survey forms enabled in the active project.",
        purpose: "\
Start here. Lists every survey form the active project has activated, with \
its slug, geometry, version, field count, what the signed-in user may do to \
it, and how many entries this desktop has cached for it. Slugs are the keys \
every other command in this domain takes.",
        effect: Effect::ReadOnly,
        authority: Authority::Project,
        execution: Execution::Sync,
        args: &[DESCRIPTOR_ARG],
        output: "\
`project`, `forms` rows with `slug`, `display_name`, `geometry_type`, `enabled`, \
`version`, `field_count`, `cached_entries`, `permissions`, and `total`; \
`orphaned` counts project-form rows whose master schema is gone.",
        examples: &[Example {
            command: "ds survey form list --output json",
            note: "Read .data.forms[].slug before any --form flag.",
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
            &crate::FORMS_LIST,
            json!({}),
            crate::READ_TIMEOUT,
        )
        .map(receipt)
        .map_err(crate::classify_survey_failure)
    }

    fn receipt(result: Value) -> Value {
        let forms: Vec<Value> = result["forms"]
            .as_array()
            .map(|rows| {
                rows.iter()
                    .map(|row| {
                        json!({
                            "slug": row["slug"],
                            "display_name": row["displayName"],
                            "geometry_type": row["geometryType"],
                            "enabled": row["enabled"],
                            "version": row["version"],
                            "field_count": row["fieldCount"],
                            "cached_entries": row["cachedEntries"],
                            "permissions": row["permissions"],
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();
        json!({
            "project": result["project"],
            "total": forms.len(),
            "forms": forms,
            "orphaned": result["orphaned"],
        })
    }

    pub fn render(data: &Value) -> String {
        let mut out = format!(
            "{}  ·  {}\n",
            data["project"].as_str().unwrap_or("?"),
            crate::plural(data["total"].as_u64().unwrap_or(0), "survey form"),
        );
        if let Some(rows) = data["forms"].as_array() {
            for row in rows {
                out.push_str(&format!(
                    "  {:<32} {:<10} v{:<4} {:>3} fields  {:>6} cached  {}\n",
                    row["slug"].as_str().unwrap_or("?"),
                    row["geometry_type"].as_str().unwrap_or("-"),
                    row["version"].as_u64().unwrap_or(0),
                    row["field_count"].as_u64().unwrap_or(0),
                    row["cached_entries"].as_u64().unwrap_or(0),
                    if row["enabled"].as_bool() == Some(false) {
                        "disabled"
                    } else {
                        ""
                    },
                ));
            }
        }
        if let Some(orphaned) = data["orphaned"].as_u64().filter(|n| *n > 0) {
            out.push_str(&format!(
                "  {} project-form row(s) point at a missing master schema\n",
                orphaned
            ));
        }
        out
    }
}

pub mod read {
    use super::*;

    pub static COMMAND: Command = Command {
        id: "survey.form.read",
        path: &["survey", "form", "read"],
        contract: 1,
        summary: "One form's schema: fields, choices, rules, permissions.",
        purpose: "\
Reads the form's Form Factory schema through the application: every field \
with its key, label, type, required flag, choices and visibility, the drawing \
and connectivity rules, the version the write commands must be issued \
against, and what the signed-in user may change. Choice lists are bounded; \
the count says when one was cut.",
        effect: Effect::ReadOnly,
        authority: Authority::Project,
        execution: Execution::Sync,
        args: &[FORM_ARG, DESCRIPTOR_ARG],
        output: "\
`slug`, `display_name`, `description`, `version`, `geometry_type`, \
`capture_detailed_location`, `connectivity_rules` when the form is a network \
element, `permissions`, `field_total`, and `fields` rows with `key`, `label`, \
`type`, `required`, `options`, `options_total`, `help_text`, `placeholder`, \
`has_visibility_rule`, `children`.",
        examples: &[Example {
            command: "ds survey form read --form edcl_customers_survey --output json",
            note: "Pass .data.fields[].key to `ds survey form field set --field`.",
            runnable: false,
        }],
        refusals: &[
            crate::NOT_PAIRED,
            crate::AMBIGUOUS,
            crate::UNREACHABLE,
            crate::PAIRING_REJECTED,
            crate::SURVEY_REFUSED,
            crate::FORM_NOT_FOUND,
            crate::INVALID_FORM,
            crate::UNSUPPORTED,
            crate::UNREADABLE,
            crate::SIGNED_OUT,
        ],
        reference: Some("docs/reference/survey.md"),
        availability: crate::paired_availability,
    };

    pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
        let form = crate::form_slug(inputs.require("form")?)?;
        let descriptor = crate::paired(inputs.value("desktop-descriptor"))?;
        crate::invoke(
            &descriptor,
            &crate::FORM_READ,
            json!({ "form": form }),
            crate::READ_TIMEOUT,
        )
        .map(receipt)
        .map_err(crate::classify_survey_failure)
    }

    pub(crate) fn field_row(row: &Value) -> Value {
        json!({
            "key": row["key"],
            "label": row["label"],
            "type": row["type"],
            "required": row["required"],
            "options": row["options"],
            "options_total": row["optionsTotal"],
            "help_text": row["helpText"],
            "placeholder": row["placeholder"],
            "has_visibility_rule": row["hasVisibilityRule"],
            "children": row["children"],
        })
    }

    fn receipt(result: Value) -> Value {
        let fields: Vec<Value> = result["fields"]
            .as_array()
            .map(|rows| rows.iter().map(field_row).collect())
            .unwrap_or_default();
        json!({
            "slug": result["slug"],
            "display_name": result["displayName"],
            "description": result["description"],
            "version": result["version"],
            "geometry_type": result["geometryType"],
            "capture_detailed_location": result["captureDetailedLocation"],
            "connectivity_rules": result["connectivityRules"],
            "permissions": result["permissions"],
            "field_total": result["fieldTotal"],
            "fields": fields,
        })
    }

    pub fn render(data: &Value) -> String {
        let mut out = format!(
            "{}  ·  v{}  ·  {}  ·  {}\n",
            data["slug"].as_str().unwrap_or("?"),
            data["version"].as_u64().unwrap_or(0),
            data["geometry_type"].as_str().unwrap_or("-"),
            crate::plural(data["field_total"].as_u64().unwrap_or(0), "field"),
        );
        if let Some(rows) = data["fields"].as_array() {
            for row in rows {
                let options = row["options"].as_array().map(|o| o.len()).unwrap_or(0);
                out.push_str(&format!(
                    "  {:<28} {:<12} {}{}{}\n",
                    row["key"].as_str().unwrap_or("?"),
                    row["type"].as_str().unwrap_or("?"),
                    if row["required"].as_bool() == Some(true) {
                        "required "
                    } else {
                        ""
                    },
                    if options > 0 {
                        format!("{} options ", options)
                    } else {
                        String::new()
                    },
                    if row["has_visibility_rule"].as_bool() == Some(true) {
                        "· conditional"
                    } else {
                        ""
                    },
                ));
            }
        }
        out
    }
}
