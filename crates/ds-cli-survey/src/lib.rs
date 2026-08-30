//! `ds survey` exposes the survey control plane without requiring a map.
//!
//! Global Form Factory schemas, project-form bindings/settings, reusable
//! project templates, and project creation from a template are related but
//! distinct lifecycle layers. Desktop control-plane commands call one closed
//! application operation. The selected-project list uses the fixed native
//! client; this crate never owns form validation or topology.

pub mod forms;
pub mod project_forms;
pub mod templates;

use std::io::Read;
use std::time::Duration;

use ds_cli_contract::Inputs;
use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{Domain, Refusal};
use ds_cli_desktop::ops::{self, BridgeOp};
use serde_json::{Map, Value, json};

pub static DOMAIN: Domain = Domain {
    id: "survey",
    summary: "Survey control plane: forms, project settings, templates, projects.",
    commands: &[
        &forms::LIST_COMMAND,
        &forms::READ_COMMAND,
        &forms::TYPES_COMMAND,
        &forms::CREATE_COMMAND,
        &forms::UPDATE_COMMAND,
        &forms::LIFECYCLE_COMMAND,
        &project_forms::READ_COMMAND,
        &project_forms::LIST_COMMAND,
        &project_forms::EDITOR_COMMAND,
        &project_forms::PLAN_COMMAND,
        &project_forms::APPLY_COMMAND,
        &templates::LIST_COMMAND,
        &templates::READ_COMMAND,
        &templates::CREATE_COMMAND,
        &templates::APPLY_COMMAND,
        &templates::LIFECYCLE_COMMAND,
        &templates::CREATE_PROJECT_COMMAND,
    ],
};

pub const TIMEOUT: Duration = Duration::from_secs(120);
const MAX_JSON_BYTES: u64 = 2 * 1024 * 1024;

pub const FORM_LIST: BridgeOp = BridgeOp {
    operation: "survey.forms.list",
    arguments: &["status", "query", "limit", "detail"],
};
pub const FORM_READ: BridgeOp = BridgeOp {
    operation: "survey.form.read",
    arguments: &["slug", "fieldOffset", "fieldLimit"],
};
pub const FORM_TYPES: BridgeOp = BridgeOp {
    operation: "survey.form.types",
    arguments: &[],
};
pub const FORM_CREATE: BridgeOp = BridgeOp {
    operation: "survey.form.create",
    arguments: &["schema"],
};
pub const FORM_UPDATE: BridgeOp = BridgeOp {
    operation: "survey.form.update",
    arguments: &["slug", "expectedVersion", "schema"],
};
pub const FORM_LIFECYCLE: BridgeOp = BridgeOp {
    operation: "survey.form.lifecycle",
    arguments: &[
        "action",
        "slug",
        "newDisplayName",
        "force",
        "expectedVersion",
    ],
};
pub const PROJECT_FORMS_READ: BridgeOp = BridgeOp {
    operation: "survey.project_forms.read",
    arguments: &["project", "status", "query", "limit", "detail"],
};
pub const PROJECT_FORM_EDITOR: BridgeOp = BridgeOp {
    operation: "survey.project_form.editor",
    arguments: &["project", "form"],
};
pub const PROJECT_FORMS_PLAN: BridgeOp = BridgeOp {
    operation: "survey.project_forms.plan",
    arguments: &["project", "changes"],
};
pub const PROJECT_FORMS_APPLY: BridgeOp = BridgeOp {
    operation: "survey.project_forms.apply",
    arguments: &["project", "changes"],
};
pub const TEMPLATE_LIST: BridgeOp = BridgeOp {
    operation: "survey.templates.list",
    arguments: &["query", "limit", "detail"],
};
pub const TEMPLATE_READ: BridgeOp = BridgeOp {
    operation: "survey.template.read",
    arguments: &["template", "formOffset", "formLimit"],
};
pub const TEMPLATE_CREATE: BridgeOp = BridgeOp {
    operation: "survey.template.create",
    arguments: &[
        "project",
        "name",
        "slug",
        "description",
        "category",
        "visibility",
    ],
};
pub const TEMPLATE_APPLY: BridgeOp = BridgeOp {
    operation: "survey.template.apply",
    arguments: &["project", "template", "mergeStrategy"],
};
pub const TEMPLATE_LIFECYCLE: BridgeOp = BridgeOp {
    operation: "survey.template.lifecycle",
    arguments: &["action", "template"],
};
pub const CREATE_PROJECT: BridgeOp = BridgeOp {
    operation: "survey.project.create_from_template",
    arguments: &["template", "projectName", "projectId"],
};

pub const BRIDGE_OPS: &[&BridgeOp] = &[
    &FORM_LIST,
    &FORM_READ,
    &FORM_TYPES,
    &FORM_CREATE,
    &FORM_UPDATE,
    &FORM_LIFECYCLE,
    &PROJECT_FORMS_READ,
    &PROJECT_FORM_EDITOR,
    &PROJECT_FORMS_PLAN,
    &PROJECT_FORMS_APPLY,
    &TEMPLATE_LIST,
    &TEMPLATE_READ,
    &TEMPLATE_CREATE,
    &TEMPLATE_APPLY,
    &TEMPLATE_LIFECYCLE,
    &CREATE_PROJECT,
];

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

pub fn invoke(
    inputs: &Inputs,
    op: &BridgeOp,
    arguments: Map<String, Value>,
) -> Result<Value, Failure> {
    let descriptor = ops::paired(inputs.value("desktop-descriptor"))?;
    ops::invoke(&descriptor, op, Value::Object(arguments), TIMEOUT)
        .map_err(ops::classify_signed_out)
}

pub const COMMON_REFUSALS: &[Refusal] = &[
    ops::NOT_PAIRED,
    ops::AMBIGUOUS,
    ops::UNREACHABLE,
    ops::PAIRING_REJECTED,
    ops::REFUSED,
    ops::UNSUPPORTED,
    ops::UNREADABLE,
    ops::INVALID_NUMBER,
    ops::SIGNED_OUT,
    INVALID_TEXT,
    INVALID_DOCUMENT,
    MISSING_REQUIRED,
];
