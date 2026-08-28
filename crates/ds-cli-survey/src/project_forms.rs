//! Per-project form bindings and settings. Explicit project ids keep this API
//! control plane independent of the map and selected-project UI state.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{Arg, Authority, Chapter, Command, Effect, Example, Execution};
use ds_cli_contract::{Context, Inputs};
use ds_cli_desktop::ops;
use serde_json::{Map, Value, json};

use crate::{
    COMMON_REFUSALS, PROJECT_FORM_EDITOR, PROJECT_FORMS_APPLY, PROJECT_FORMS_PLAN,
    PROJECT_FORMS_READ,
};

pub static READ_COMMAND: Command = Command {
    id: "survey.project-forms.read",
    path: &["survey", "project-forms", "read"],
    contract: 1,
    summary: "Read one project's form bindings and effective settings.",
    purpose: "Activates the canonical project-forms resolution through the signed-in API session. It returns enabled and disabled bindings, orphan status, permissions, revisions and optionally bounded effective settings; no map is opened.",
    chapter: Chapter::Survey,
    effect: Effect::ReadOnly,
    authority: Authority::DesktopUser,
    execution: Execution::Sync,
    args: &[
        Arg::value(
            "project",
            "<project-id>",
            "Exact Data Solutions project id.",
        )
        .required(),
        Arg::value("status", "<status>", "Filter project-form bindings.")
            .choices(&["enabled", "disabled", "unavailable", "all"])
            .default("all"),
        Arg::value("query", "<text>", "Match form slug, name, or description."),
        Arg::value("limit", "<n>", "Return at most 1..500 bindings.").default("50"),
        Arg::switch(
            "detail",
            "Include project settings, capabilities and permissions.",
        ),
        ops::DESCRIPTOR_ARG,
    ],
    output: "The exact project id, matching total, bounded project-form rows, orphan bindings, field vocabulary metadata, and omitted count.",
    examples: &[Example {
        command: "ds survey project-forms read --project nyamata --status enabled --detail --output json",
        note: "Discover which forms and network settings are active without opening the map.",
        runnable: false,
    }],
    refusals: COMMON_REFUSALS,
    reference: Some("docs/reference/survey.md"),
    availability: ops::paired_availability,
};

pub static EDITOR_COMMAND: Command = Command {
    id: "survey.project-form.editor",
    path: &["survey", "project-form", "editor"],
    contract: 1,
    summary: "Read the backend-owned settings editor for one project form.",
    purpose: "Returns current and effective settings, typed editor sections, field state, capabilities and the optimistic settings revision. Unavailable master forms remain readable for cleanup.",
    chapter: Chapter::Survey,
    effect: Effect::ReadOnly,
    authority: Authority::DesktopUser,
    execution: Execution::Sync,
    args: &[
        Arg::value(
            "project",
            "<project-id>",
            "Exact Data Solutions project id.",
        )
        .required(),
        Arg::value("form", "<form-slug>", "Exact project-form slug.").required(),
        ops::DESCRIPTOR_ARG,
    ],
    output: "The canonical settings editor, including current/effective settings and version for a later plan or apply.",
    examples: &[Example {
        command: "ds survey project-form editor --project nyamata --form lv_poles_survey --output json",
        note: "Learn the legal network-setting keys and current revision before drafting changes.",
        runnable: false,
    }],
    refusals: COMMON_REFUSALS,
    reference: Some("docs/reference/survey.md"),
    availability: ops::paired_availability,
};

pub static PLAN_COMMAND: Command = Command {
    id: "survey.project-forms.plan",
    path: &["survey", "project-forms", "plan"],
    contract: 1,
    summary: "Validate a staged project-form settings batch without saving.",
    purpose: "Compares a bounded JSON change array with live bindings and settings editors. It checks identities, editable keys and optimistic revisions but performs no bulk-save mutation.",
    chapter: Chapter::Survey,
    effect: Effect::Proposal,
    authority: Authority::DesktopUser,
    execution: Execution::Sync,
    args: &[
        Arg::value(
            "project",
            "<project-id>",
            "Exact Data Solutions project id.",
        )
        .required(),
        Arg::value(
            "changes",
            "<json-path>",
            "JSON array of form_slug, optional enabled, settings, and expected_version.",
        )
        .required(),
        ops::DESCRIPTOR_ARG,
    ],
    output: "A non-writing plan naming every requested form, validation result, revision comparison, changed setting keys, and whether apply is ready.",
    examples: &[Example {
        command: "ds survey project-forms plan --project nyamata --changes ./network-form-settings.json --output json",
        note: "Hypothetically enable node/edge forms and verify their topology settings before saving.",
        runnable: false,
    }],
    refusals: COMMON_REFUSALS,
    reference: Some("docs/reference/survey.md"),
    availability: ops::paired_availability,
};

pub static APPLY_COMMAND: Command = Command {
    id: "survey.project-forms.apply",
    path: &["survey", "project-forms", "apply"],
    contract: 1,
    summary: "Atomically save a planned project-form settings batch.",
    purpose: "Uses the canonical bulk-save transaction. Every settings-bearing row must carry the editor revision it was based on; enable-only rows preserve settings and do not manufacture settings conflicts.",
    chapter: Chapter::Survey,
    effect: Effect::GlobalWrite,
    authority: Authority::DesktopUser,
    execution: Execution::Sync,
    args: &[
        Arg::value(
            "project",
            "<project-id>",
            "Exact Data Solutions project id.",
        )
        .required(),
        Arg::value(
            "changes",
            "<json-path>",
            "The same JSON change array reviewed with project-forms plan.",
        )
        .required(),
        ops::DESCRIPTOR_ARG,
    ],
    output: "The verified bulk-save receipt: saved rows, normalized/effective settings, cleanup outcomes, and refreshed bindings when available.",
    examples: &[Example {
        command: "ds survey project-forms apply --project nyamata --changes ./network-form-settings.json --yes --output json",
        note: "Apply only the exact batch whose plan reported ready.",
        runnable: false,
    }],
    refusals: COMMON_REFUSALS,
    reference: Some("docs/reference/survey.md"),
    availability: ops::paired_availability,
};

fn base(inputs: &Inputs) -> Result<Map<String, Value>, Failure> {
    Ok(Map::from_iter([(
        "project".into(),
        json!(crate::text(inputs.require("project")?, "project", 160)?),
    )]))
}

pub fn read(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let mut args = base(inputs)?;
    args.insert("status".into(), json!(inputs.require("status")?));
    args.insert(
        "limit".into(),
        json!(ops::integer(inputs.require("limit")?, "limit", 1, 500)?),
    );
    crate::optional_text(inputs, "query", "query", 200, &mut args)?;
    if inputs.switch("detail") {
        args.insert("detail".into(), json!(true));
    }
    crate::invoke(inputs, &PROJECT_FORMS_READ, args)
}

pub fn editor(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let mut args = base(inputs)?;
    args.insert(
        "form".into(),
        json!(crate::text(inputs.require("form")?, "form", 160)?),
    );
    crate::invoke(inputs, &PROJECT_FORM_EDITOR, args)
}

fn changes(inputs: &Inputs, op: &ds_cli_desktop::ops::BridgeOp) -> Result<Value, Failure> {
    let mut args = base(inputs)?;
    args.insert(
        "changes".into(),
        crate::load_json(inputs.require("changes")?, "changes", true)?,
    );
    crate::invoke(inputs, op, args)
}

pub fn plan(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    changes(inputs, &PROJECT_FORMS_PLAN)
}

pub fn apply(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    changes(inputs, &PROJECT_FORMS_APPLY)
}

pub fn render_read(data: &Value) -> String {
    format!(
        "{}  {} project forms\n",
        data["project"].as_str().unwrap_or("project"),
        data["total"].as_u64().unwrap_or(0)
    )
}

pub fn render_editor(data: &Value) -> String {
    format!(
        "{}/{}  settings v{}\n",
        data["project"].as_str().unwrap_or("project"),
        data["form"].as_str().unwrap_or("form"),
        data["editor"]["version"].as_u64().unwrap_or(0)
    )
}

pub fn render_plan(data: &Value) -> String {
    format!(
        "{} changes  {}\n",
        data["changes"].as_array().map(Vec::len).unwrap_or(0),
        if data["ready"].as_bool().unwrap_or(false) {
            "ready"
        } else {
            "not ready"
        }
    )
}

pub fn render_apply(data: &Value) -> String {
    format!(
        "{} project forms saved\n",
        data["applied"].as_u64().unwrap_or(0)
    )
}
