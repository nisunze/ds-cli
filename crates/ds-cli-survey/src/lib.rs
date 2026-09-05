//! `ds survey` exposes the survey control plane without requiring a map.
//!
//! Global Form Factory schemas, project-form bindings/settings, reusable
//! project templates, and project creation from a template are related but
//! distinct lifecycle layers. Every control-plane operation uses the fixed
//! native client. Offline workspaces use their Rust domain owner; this crate
//! owns argument parsing and IO, never form validation or topology.

pub mod changes;
pub mod create;
pub mod entries;
pub mod forms;
pub mod import;
pub mod project_forms;
pub mod query;
pub mod templates;
pub mod workspace;

use std::io::Read;

use ds_cli_contract::Inputs;
use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{Domain, Refusal};
use ds_cli_desktop::ops::BridgeOp;
use serde_json::{Map, Value, json};

pub static DOMAIN: Domain = Domain {
    id: "survey",
    summary: "Survey control plane: forms, project settings, templates, projects.",
    commands: &[
        &workspace::INIT,
        &workspace::PREPARE,
        &workspace::COLLECT,
        &workspace::LIST,
        &workspace::SYNC,
        &forms::LIST_COMMAND,
        &forms::READ_COMMAND,
        &forms::TYPES_COMMAND,
        &forms::CREATE_COMMAND,
        &forms::UPDATE_COMMAND,
        &forms::LIFECYCLE_COMMAND,
        &project_forms::READ_COMMAND,
        &project_forms::LIST_COMMAND,
        &project_forms::SETTINGS_COMMAND,
        &project_forms::EDITOR_COMMAND,
        &project_forms::PLAN_COMMAND,
        &project_forms::APPLY_COMMAND,
        &query::COMMAND,
        &entries::COMMAND,
        &changes::COMMAND,
        &create::COMMAND,
        &import::COMMAND,
        &templates::LIST_COMMAND,
        &templates::READ_COMMAND,
        &templates::CREATE_COMMAND,
        &templates::APPLY_COMMAND,
        &templates::LIFECYCLE_COMMAND,
        &templates::CREATE_PROJECT_COMMAND,
    ],
};

const MAX_JSON_BYTES: u64 = 2 * 1024 * 1024;

pub const FORM_LIST: &str = "survey.forms.list";
pub const FORM_READ: &str = "survey.form.read";
pub const FORM_TYPES: &str = "survey.form.types";
pub const FORM_CREATE: &str = "survey.form.create";
pub const FORM_UPDATE: &str = "survey.form.update";
pub const FORM_LIFECYCLE: &str = "survey.form.lifecycle";
pub const PROJECT_FORMS_READ: &str = "survey.project_forms.read";
pub const PROJECT_FORM_EDITOR: &str = "survey.project_form.editor";
pub const PROJECT_FORMS_PLAN: &str = "survey.project_forms.plan";
pub const PROJECT_FORMS_APPLY: &str = "survey.project_forms.apply";
pub const TEMPLATE_LIST: &str = "survey.templates.list";
pub const TEMPLATE_READ: &str = "survey.template.read";
pub const TEMPLATE_CREATE: &str = "survey.template.create";
pub const TEMPLATE_APPLY: &str = "survey.template.apply";
pub const TEMPLATE_LIFECYCLE: &str = "survey.template.lifecycle";
pub const CREATE_PROJECT: &str = "survey.project.create_from_template";

pub const BRIDGE_OPS: &[&BridgeOp] = &[];

pub const INVALID_TEXT: Refusal = Refusal {
    code: "invalid_text",
    when: "an identity or label is empty, untrimmed, or exceeds its bound",
    remedy: "use the exact bounded slug, project id, template id, or label",
};
pub const INVALID_DOCUMENT: Refusal = Refusal {
    code: "invalid_document",
    when: "a schema or settings-change file is missing, oversized, invalid JSON, or the wrong shape",
    remedy: "pass a UTF-8 JSON object for a form schema or a JSON array for project-form changes",
};
pub const MISSING_REQUIRED: Refusal = Refusal {
    code: "missing_required",
    when: "the selected lifecycle action requires an action-specific argument",
    remedy: "read the command contract and provide the argument required by that action",
};

pub fn text<'a>(value: &'a str, flag: &str, max: usize) -> Result<&'a str, Failure> {
    if value.is_empty() || value.trim() != value || value.chars().count() > max {
        return Err(Failure::invalid(
            INVALID_TEXT.code,
            format!("`--{flag}` must be non-empty, trimmed, and at most {max} characters"),
        )
        .remedy(INVALID_TEXT.remedy));
    }
    Ok(value)
}

pub fn optional_text(
    inputs: &Inputs,
    flag: &str,
    target: &str,
    max: usize,
    arguments: &mut Map<String, Value>,
) -> Result<(), Failure> {
    if let Some(value) = inputs.value(flag) {
        arguments.insert(target.into(), json!(text(value, flag, max)?));
    }
    Ok(())
}

pub fn load_json(raw: &str, flag: &str, array: bool) -> Result<Value, Failure> {
    let path = std::path::Path::new(raw);
    let metadata = std::fs::metadata(path).map_err(|error| {
        Failure::invalid(
            INVALID_DOCUMENT.code,
            format!("`{raw}` is not a readable file"),
        )
        .remedy(INVALID_DOCUMENT.remedy)
        .detail(json!({ "detail": error.kind().to_string() }))
    })?;
    if !metadata.is_file() || metadata.len() > MAX_JSON_BYTES {
        return Err(Failure::invalid(
            INVALID_DOCUMENT.code,
            format!(
                "`--{flag}` must name a regular JSON file no larger than {MAX_JSON_BYTES} bytes"
            ),
        )
        .remedy(INVALID_DOCUMENT.remedy));
    }
    let mut body = String::new();
    std::fs::File::open(path)
        .and_then(|file| file.take(MAX_JSON_BYTES + 1).read_to_string(&mut body))
        .map_err(|error| {
            Failure::invalid(INVALID_DOCUMENT.code, format!("`{raw}` could not be read"))
                .remedy(INVALID_DOCUMENT.remedy)
                .detail(json!({ "detail": error.kind().to_string() }))
        })?;
    let value: Value = serde_json::from_str(&body).map_err(|error| {
        Failure::invalid(INVALID_DOCUMENT.code, format!("`{raw}` is not valid JSON"))
            .remedy(INVALID_DOCUMENT.remedy)
            .detail(json!({ "detail": error.to_string().chars().take(160).collect::<String>() }))
    })?;
    if (array && !value.is_array()) || (!array && !value.is_object()) {
        return Err(Failure::invalid(
            INVALID_DOCUMENT.code,
            format!("`--{flag}` has the wrong JSON shape"),
        )
        .remedy(INVALID_DOCUMENT.remedy));
    }
    Ok(value)
}

pub fn invoke(inputs: &Inputs, op: &str, arguments: Map<String, Value>) -> Result<Value, Failure> {
    let command: ds_client_core::SurveyControlCommand =
        serde_json::from_value(json!({"operation":op,"arguments":arguments})).map_err(|_| {
            Failure::invalid(
                "invalid_document",
                "Survey arguments violate the typed native contract",
            )
            .remedy(INVALID_DOCUMENT.remedy)
        })?;
    ds_cli_auth::survey_control(inputs.value("lane").unwrap_or("stable"), &command)
}

pub const LANE: ds_cli_contract::spec::Arg =
    ds_cli_contract::spec::Arg::value("lane", "<stable|canary>", "Native deployment lane.")
        .default("stable")
        .choices(&["stable", "canary"]);
pub const COMMON_REFUSALS: &[Refusal] = &{
    const BASE: &[Refusal] = project_forms::LIST_COMMAND.refusals;
    let mut list = [INVALID_TEXT; BASE.len() + 3];
    let mut i = 0;
    while i < BASE.len() {
        list[i] = BASE[i];
        i += 1;
    }
    list[BASE.len()] = INVALID_TEXT;
    list[BASE.len() + 1] = INVALID_DOCUMENT;
    list[BASE.len() + 2] = MISSING_REQUIRED;
    list
};

#[cfg(test)]
mod availability_tests {
    use super::*;
    use ds_cli_contract::spec::Availability;

    #[test]
    fn native_survey_descriptors_share_the_protected_state_gate() {
        let expected = ds_cli_auth::native_availability as fn() -> Availability;
        for command in [&entries::COMMAND, &changes::COMMAND, &create::COMMAND] {
            assert!(std::ptr::fn_addr_eq(command.availability, expected));
        }
        assert!(!std::ptr::fn_addr_eq(
            import::COMMAND.availability,
            expected
        ));
    }
}
