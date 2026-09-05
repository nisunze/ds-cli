//! Global Form Factory commands. These never require a map or active project.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{Arg, Authority, Chapter, Command, Effect, Example, Execution};
use ds_cli_contract::{Context, Inputs};
use ds_cli_desktop::ops;
use serde_json::{Map, Value, json};

use crate::{
    COMMON_REFUSALS, FORM_CREATE, FORM_LIFECYCLE, FORM_LIST, FORM_READ, FORM_TYPES, FORM_UPDATE,
};

pub static LIST_COMMAND: Command = Command {
    id: "survey.forms.list",
    path: &["survey", "forms", "list"],
    contract: 2,
    summary: "List global Form Factory schemas without opening the map.",
    purpose: "Reads the governed Form Factory catalogue through the signed-in API session. Returns bounded schema summaries by default; --detail adds authoring metadata without returning every field.",
    chapter: Chapter::Survey,
    effect: Effect::LocalAuthState,
    authority: Authority::HeadlessUser,
    execution: Execution::Sync,
    args: &[
        Arg::value("status", "<status>", "Filter the global catalogue.")
            .choices(&["active", "archived", "all"])
            .default("active"),
        Arg::value(
            "query",
            "<text>",
            "Match slug, display name, or description.",
        ),
        Arg::value("limit", "<n>", "Return at most 1..500 schemas.").default("50"),
        Arg::switch("detail", "Include bounded schema metadata in each row."),
        crate::LANE,
    ],
    output: "The filter, matching total, bounded form rows, and omitted count. Rows name slug, display name, version, visibility, geometry, and field count.",
    examples: &[Example {
        command: "ds survey forms list --query pole --detail --output json",
        note: "A hypothetical request: which pole forms could participate in a network survey?",
        runnable: false,
    }],
    refusals: COMMON_REFUSALS,
    reference: Some("docs/reference/survey.md"),
    availability: ds_cli_auth::native_availability,
};

pub static READ_COMMAND: Command = Command {
    id: "survey.form.read",
    path: &["survey", "form", "read"],
    contract: 2,
    summary: "Read one Form Factory schema and a bounded field page.",
    purpose: "Reads one global master form, including its drawing, visibility, validation and field definitions. Field pagination keeps complex forms discoverable without flooding one response.",
    chapter: Chapter::Survey,
    effect: Effect::LocalAuthState,
    authority: Authority::HeadlessUser,
    execution: Execution::Sync,
    args: &[
        Arg::value("slug", "<form-slug>", "Exact global form slug.").required(),
        Arg::value("field-offset", "<n>", "Zero-based field offset.").default("0"),
        Arg::value("field-limit", "<n>", "Return 1..100 fields.").default("50"),
        crate::LANE,
    ],
    output: "The canonical form schema with one ordered field page, total field count, next offset, and completeness flag.",
    examples: &[Example {
        command: "ds survey form read --slug lv_poles_survey --field-limit 25 --output json",
        note: "Inspect the master schema before changing a project binding or template.",
        runnable: false,
    }],
    refusals: COMMON_REFUSALS,
    reference: Some("docs/reference/survey.md"),
    availability: ds_cli_auth::native_availability,
};

pub static TYPES_COMMAND: Command = Command {
    id: "survey.form.types",
    path: &["survey", "form", "types"],
    contract: 2,
    summary: "Read the Form Factory field-type and condition registry.",
    purpose: "Returns the backend-owned authoring vocabulary an LLM needs before proposing a form schema. It does not infer types from existing forms.",
    chapter: Chapter::Survey,
    effect: Effect::LocalAuthState,
    authority: Authority::HeadlessUser,
    execution: Execution::Sync,
    args: &[crate::LANE],
    output: "Field types, categories, condition operators and type aliases from Form Factory.",
    examples: &[Example {
        command: "ds survey form types --output json",
        note: "Discover legal field vocabulary before authoring a hypothetical inspection form.",
        runnable: false,
    }],
    refusals: COMMON_REFUSALS,
    reference: Some("docs/reference/survey.md"),
    availability: ds_cli_auth::native_availability,
};

pub static CREATE_COMMAND: Command = Command {
    id: "survey.form.create",
    path: &["survey", "form", "create"],
    contract: 2,
    summary: "Create one global Form Factory schema from bounded JSON.",
    purpose: "Sends one explicit schema document to Form Factory. ds-brain remains the authority for slug normalization, field validation, geometry rules and authoring permission.",
    chapter: Chapter::Survey,
    effect: Effect::GlobalWrite,
    authority: Authority::HeadlessUser,
    execution: Execution::Sync,
    args: &[
        Arg::value(
            "schema",
            "<json-path>",
            "UTF-8 JSON object containing the proposed form schema.",
        )
        .required(),
        crate::LANE,
    ],
    output: "The created canonical form with its assigned slug and version.",
    examples: &[Example {
        command: "ds survey form create --schema ./water-valve-inspection.json --yes --output json",
        note: "Hypothetical only: inspect field types first, then let Form Factory validate the schema.",
        runnable: false,
    }],
    refusals: COMMON_REFUSALS,
    reference: Some("docs/reference/survey.md"),
    availability: ds_cli_auth::native_availability,
};

pub static UPDATE_COMMAND: Command = Command {
    id: "survey.form.update",
    path: &["survey", "form", "update"],
    contract: 2,
    summary: "Update one global form with an optimistic version.",
    purpose: "Applies one schema patch to the named Form Factory master. The expected version prevents silently replacing a form changed since it was read.",
    chapter: Chapter::Survey,
    effect: Effect::GlobalWrite,
    authority: Authority::HeadlessUser,
    execution: Execution::Sync,
    args: &[
        Arg::value("slug", "<form-slug>", "Exact global form slug.").required(),
        Arg::value(
            "expect-version",
            "<version>",
            "Version returned by survey form read.",
        )
        .required(),
        Arg::value(
            "schema",
            "<json-path>",
            "UTF-8 JSON object containing the intended schema patch.",
        )
        .required(),
        crate::LANE,
    ],
    output: "The updated canonical form and its advanced version.",
    examples: &[Example {
        command: "ds survey form update --slug valve_inspection --expect-version 4 --schema ./valve-patch.json --yes --output json",
        note: "A hypothetical modification; read version 4 immediately before applying it.",
        runnable: false,
    }],
    refusals: COMMON_REFUSALS,
    reference: Some("docs/reference/survey.md"),
    availability: ds_cli_auth::native_availability,
};

pub static LIFECYCLE_COMMAND: Command = Command {
    id: "survey.form.lifecycle",
    path: &["survey", "form", "lifecycle"],
    contract: 2,
    summary: "Duplicate, publish, archive, restore, or delete one master form.",
    purpose: "Projects one closed Form Factory lifecycle action. Archive and delete remain dependency-aware and refuse live project or template bindings unless --force expresses that exact destructive intent.",
    chapter: Chapter::Survey,
    effect: Effect::GlobalWrite,
    authority: Authority::HeadlessUser,
    execution: Execution::Sync,
    args: &[
        Arg::value("action", "<action>", "The one lifecycle transition.")
            .choices(&[
                "duplicate",
                "publish",
                "unpublish",
                "archive",
                "restore",
                "delete",
            ])
            .required(),
        Arg::value("slug", "<form-slug>", "Exact global form slug.").required(),
        Arg::value("new-display-name", "<name>", "Required only for duplicate."),
        Arg::value(
            "expect-version",
            "<version>",
            "Optional optimistic version for visibility changes.",
        ),
        Arg::switch(
            "force",
            "Allow archive/delete to leave project or template bindings unavailable.",
        ),
        crate::LANE,
    ],
    output: "The lifecycle action and the backend's canonical resulting form or mutation receipt.",
    examples: &[Example {
        command: "ds survey form lifecycle --action duplicate --slug pole_survey --new-display-name 'Pole Survey Trial' --yes --output json",
        note: "Duplicate before experimenting with a complex production form.",
        runnable: false,
    }],
    refusals: COMMON_REFUSALS,
    reference: Some("docs/reference/survey.md"),
    availability: ds_cli_auth::native_availability,
};

pub fn list(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let mut args = Map::from_iter([
        ("status".into(), json!(inputs.require("status")?)),
        (
            "limit".into(),
            json!(ops::integer(inputs.require("limit")?, "limit", 1, 500)?),
        ),
    ]);
    crate::optional_text(inputs, "query", "query", 200, &mut args)?;
    if inputs.switch("detail") {
        args.insert("detail".into(), json!(true));
    }
    crate::invoke(inputs, FORM_LIST, args)
}

pub fn read(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let args = Map::from_iter([
        (
            "slug".into(),
            json!(crate::text(inputs.require("slug")?, "slug", 160)?),
        ),
        (
            "fieldOffset".into(),
            json!(ops::integer(
                inputs.require("field-offset")?,
                "field-offset",
                0,
                100_000
            )?),
        ),
        (
            "fieldLimit".into(),
            json!(ops::integer(
                inputs.require("field-limit")?,
                "field-limit",
                1,
                100
            )?),
        ),
    ]);
    crate::invoke(inputs, FORM_READ, args)
}

pub fn types(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    crate::invoke(inputs, FORM_TYPES, Map::new())
}

pub fn create(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let schema = crate::load_json(inputs.require("schema")?, "schema", false)?;
    crate::invoke(
        inputs,
        FORM_CREATE,
        Map::from_iter([("schema".into(), schema)]),
    )
}

pub fn update(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let args = Map::from_iter([
        (
            "slug".into(),
            json!(crate::text(inputs.require("slug")?, "slug", 160)?),
        ),
        (
            "expectedVersion".into(),
            json!(ops::integer(
                inputs.require("expect-version")?,
                "expect-version",
                1,
                i64::MAX
            )?),
        ),
        (
            "schema".into(),
            crate::load_json(inputs.require("schema")?, "schema", false)?,
        ),
    ]);
    crate::invoke(inputs, FORM_UPDATE, args)
}

pub fn lifecycle(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let action = inputs.require("action")?;
    let mut args = Map::from_iter([
        ("action".into(), json!(action)),
        (
            "slug".into(),
            json!(crate::text(inputs.require("slug")?, "slug", 160)?),
        ),
    ]);
    crate::optional_text(inputs, "new-display-name", "newDisplayName", 200, &mut args)?;
    if action == "duplicate" && !args.contains_key("newDisplayName") {
        return Err(
            Failure::invalid("missing_required", "duplicate requires --new-display-name")
                .remedy("name the new form without changing the source form"),
        );
    }
    if let Some(version) = inputs.value("expect-version") {
        args.insert(
            "expectedVersion".into(),
            json!(ops::integer(version, "expect-version", 1, i64::MAX)?),
        );
    }
    if inputs.switch("force") {
        args.insert("force".into(), json!(true));
    }
    crate::invoke(inputs, FORM_LIFECYCLE, args)
}

pub fn render_list(data: &Value) -> String {
    format!(
        "{} forms matched; {} returned\n",
        data["total"].as_u64().unwrap_or(0),
        data["forms"].as_array().map(Vec::len).unwrap_or(0)
    )
}

pub fn render_form(data: &Value) -> String {
    let form = data.get("form").unwrap_or(data);
    format!(
        "{}  v{}  {} fields\n",
        form["slug"].as_str().unwrap_or("form"),
        form["version"].as_u64().unwrap_or(0),
        data["fieldTotal"]
            .as_u64()
            .or_else(|| form["fields"].as_array().map(|v| v.len() as u64))
            .unwrap_or(0)
    )
}

pub fn render_types(data: &Value) -> String {
    format!(
        "{} field types\n",
        data["fieldTypes"].as_array().map(Vec::len).unwrap_or(0)
    )
}

pub fn render_lifecycle(data: &Value) -> String {
    format!(
        "{} {}\n",
        data["action"].as_str().unwrap_or("form"),
        data["slug"]
            .as_str()
            .or_else(|| data["form"]["slug"].as_str())
            .unwrap_or("")
    )
}
