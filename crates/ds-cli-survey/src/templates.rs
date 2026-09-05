//! Reusable project-template commands and the separate new-project creation
//! operation. Templates are snapshots; projects are independent instances.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{Arg, Authority, Chapter, Command, Effect, Example, Execution};
use ds_cli_contract::{Context, Inputs};
use ds_cli_desktop::ops;
use serde_json::{Map, Value, json};

use crate::{
    COMMON_REFUSALS, CREATE_PROJECT, TEMPLATE_APPLY, TEMPLATE_CREATE, TEMPLATE_LIFECYCLE,
    TEMPLATE_LIST, TEMPLATE_READ,
};

pub static LIST_COMMAND: Command = Command {
    id: "survey.templates.list",
    path: &["survey", "templates", "list"],
    contract: 2,
    summary: "List reusable project templates and their form counts.",
    purpose: "Reads reusable project-configuration snapshots. These are not Form Factory masters and are not projects created from them.",
    chapter: Chapter::Survey,
    effect: Effect::LocalAuthState,
    authority: Authority::HeadlessUser,
    execution: Execution::Sync,
    args: &[
        Arg::value(
            "query",
            "<text>",
            "Match template slug, name, description, or category.",
        ),
        Arg::value("limit", "<n>", "Return at most 1..500 templates.").default("50"),
        Arg::switch("detail", "Include bounded template metadata."),
        crate::LANE,
    ],
    output: "Matching total, bounded template rows, form counts, visibility, source project, and omitted count.",
    examples: &[Example {
        command: "ds survey templates list --query utility --detail --output json",
        note: "Find a reusable configuration before applying it or creating a project.",
        runnable: false,
    }],
    refusals: COMMON_REFUSALS,
    reference: Some("docs/reference/survey.md"),
    availability: ds_cli_auth::native_availability,
};

pub static READ_COMMAND: Command = Command {
    id: "survey.template.read",
    path: &["survey", "template", "read"],
    contract: 2,
    summary: "Read one project template and a bounded project-form page.",
    purpose: "Returns the reusable template snapshot, including its network summary and paged per-form settings. It does not activate or create a project.",
    chapter: Chapter::Survey,
    effect: Effect::LocalAuthState,
    authority: Authority::HeadlessUser,
    execution: Execution::Sync,
    args: &[
        Arg::value(
            "template",
            "<template-slug>",
            "Exact project-template slug.",
        )
        .required(),
        Arg::value("form-offset", "<n>", "Zero-based template-form offset.").default("0"),
        Arg::value("form-limit", "<n>", "Return 1..100 template forms.").default("50"),
        crate::LANE,
    ],
    output: "The template metadata and one ordered project-form settings page with total, next offset and completeness.",
    examples: &[Example {
        command: "ds survey template read --template utility_survey --form-limit 25 --output json",
        note: "Inspect what a project created from this template would receive.",
        runnable: false,
    }],
    refusals: COMMON_REFUSALS,
    reference: Some("docs/reference/survey.md"),
    availability: ds_cli_auth::native_availability,
};

pub static CREATE_COMMAND: Command = Command {
    id: "survey.template.create",
    path: &["survey", "template", "create"],
    contract: 2,
    summary: "Create a reusable project template from one project's forms.",
    purpose: "Snapshots the named project's canonical project-form configuration into a distinct reusable template. It does not create another project.",
    chapter: Chapter::Survey,
    effect: Effect::GlobalWrite,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[
        Arg::value(
            "project",
            "<project-id>",
            "Source project whose form configuration is snapshotted.",
        )
        .required(),
        Arg::value("name", "<name>", "Template display name.").required(),
        Arg::value("slug", "<template-slug>", "Optional exact template slug."),
        Arg::value("description", "<text>", "Optional template description."),
        Arg::value("category", "<category>", "Optional catalogue category."),
        Arg::value("visibility", "<visibility>", "Initial template visibility.")
            .choices(&["private", "organization", "public"])
            .default("private"),
        crate::LANE,
    ],
    output: "The created template slug and canonical template snapshot.",
    examples: &[Example {
        command: "ds survey template create --project nyamata --name 'Utility Survey v2' --category utility --yes --output json",
        note: "Create a template from the project; this does not create a project from the template.",
        runnable: false,
    }],
    refusals: COMMON_REFUSALS,
    reference: Some("docs/reference/survey.md"),
    availability: ds_cli_auth::native_availability,
};

pub static APPLY_COMMAND: Command = Command {
    id: "survey.template.apply",
    path: &["survey", "template", "apply"],
    contract: 2,
    summary: "Apply a project template to an existing project.",
    purpose: "Writes template-contained project-form settings into the explicitly named existing project. This is not new-project creation.",
    chapter: Chapter::Survey,
    effect: Effect::GlobalWrite,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[
        Arg::value("project", "<project-id>", "Existing target project id.").required(),
        Arg::value("template", "<template-slug>", "Reusable template to apply.").required(),
        Arg::value(
            "merge-strategy",
            "<strategy>",
            "How existing project-form rows are handled.",
        )
        .choices(&["overwrite", "preserve"])
        .default("overwrite"),
        crate::LANE,
    ],
    output: "The target project, forms applied/skipped, and refreshed project-form state when available.",
    examples: &[Example {
        command: "ds survey template apply --project nyamata --template utility_survey --merge-strategy preserve --yes --output json",
        note: "Apply to an existing project; use project create-from-template for a new project.",
        runnable: false,
    }],
    refusals: COMMON_REFUSALS,
    reference: Some("docs/reference/survey.md"),
    availability: ds_cli_auth::native_availability,
};

pub static LIFECYCLE_COMMAND: Command = Command {
    id: "survey.template.lifecycle",
    path: &["survey", "template", "lifecycle"],
    contract: 2,
    summary: "Publish, privatize, or delete one reusable project template.",
    purpose: "Changes only the template catalogue object. Projects previously created from or updated by it remain independent project instances.",
    chapter: Chapter::Survey,
    effect: Effect::GlobalWrite,
    authority: Authority::HeadlessUser,
    execution: Execution::Sync,
    args: &[
        Arg::value("action", "<action>", "The one template transition.")
            .choices(&["publish", "privatize", "delete"])
            .required(),
        Arg::value(
            "template",
            "<template-slug>",
            "Exact reusable template slug.",
        )
        .required(),
        crate::LANE,
    ],
    output: "The requested transition and backend mutation receipt.",
    examples: &[Example {
        command: "ds survey template lifecycle --action publish --template utility_survey --yes --output json",
        note: "Publishing a template does not publish or modify its master forms.",
        runnable: false,
    }],
    refusals: COMMON_REFUSALS,
    reference: Some("docs/reference/survey.md"),
    availability: ds_cli_auth::native_availability,
};

pub static CREATE_PROJECT_COMMAND: Command = Command {
    id: "survey.project.create-from-template",
    path: &["survey", "project", "create-from-template"],
    contract: 2,
    summary: "Create a new project instance from a reusable project template.",
    purpose: "Creates a distinct project and copies the template's project-form configuration into it. It does not modify the template and does not require a map or active project.",
    chapter: Chapter::Survey,
    effect: Effect::GlobalWrite,
    authority: Authority::HeadlessUser,
    execution: Execution::Sync,
    args: &[
        Arg::value(
            "template",
            "<template-slug>",
            "Reusable template to instantiate.",
        )
        .required(),
        Arg::value(
            "project-name",
            "<name>",
            "Display name for the new project.",
        )
        .required(),
        Arg::value(
            "project-id",
            "<project-id>",
            "Optional exact id for the new project.",
        ),
        crate::LANE,
    ],
    output: "The new project id and name, source template slug, and number of project forms enabled.",
    examples: &[Example {
        command: "ds survey project create-from-template --template utility_survey --project-name 'Nyamata Water Trial' --yes --output json",
        note: "Hypothetical utility example: creates a project; it does not create or alter the template.",
        runnable: false,
    }],
    refusals: COMMON_REFUSALS,
    reference: Some("docs/reference/survey.md"),
    availability: ds_cli_auth::native_availability,
};

pub fn list(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let mut args = Map::from_iter([(
        "limit".into(),
        json!(ops::integer(inputs.require("limit")?, "limit", 1, 500)?),
    )]);
    crate::optional_text(inputs, "query", "query", 200, &mut args)?;
    if inputs.switch("detail") {
        args.insert("detail".into(), json!(true));
    }
    crate::invoke(inputs, TEMPLATE_LIST, args)
}

pub fn read(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let args = Map::from_iter([
        (
            "template".into(),
            json!(crate::text(inputs.require("template")?, "template", 160)?),
        ),
        (
            "formOffset".into(),
            json!(ops::integer(
                inputs.require("form-offset")?,
                "form-offset",
                0,
                100_000
            )?),
        ),
        (
            "formLimit".into(),
            json!(ops::integer(
                inputs.require("form-limit")?,
                "form-limit",
                1,
                100
            )?),
        ),
    ]);
    crate::invoke(inputs, TEMPLATE_READ, args)
}

pub fn create(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let mut args = Map::from_iter([
        (
            "project".into(),
            json!(crate::text(inputs.require("project")?, "project", 160)?),
        ),
        (
            "name".into(),
            json!(crate::text(inputs.require("name")?, "name", 200)?),
        ),
        ("visibility".into(), json!(inputs.require("visibility")?)),
    ]);
    crate::optional_text(inputs, "slug", "slug", 160, &mut args)?;
    crate::optional_text(inputs, "description", "description", 2_000, &mut args)?;
    crate::optional_text(inputs, "category", "category", 100, &mut args)?;
    crate::invoke(inputs, TEMPLATE_CREATE, args)
}

pub fn apply(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let args = Map::from_iter([
        (
            "project".into(),
            json!(crate::text(inputs.require("project")?, "project", 160)?),
        ),
        (
            "template".into(),
            json!(crate::text(inputs.require("template")?, "template", 160)?),
        ),
        (
            "mergeStrategy".into(),
            json!(inputs.require("merge-strategy")?),
        ),
    ]);
    crate::invoke(inputs, TEMPLATE_APPLY, args)
}

pub fn lifecycle(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let args = Map::from_iter([
        ("action".into(), json!(inputs.require("action")?)),
        (
            "template".into(),
            json!(crate::text(inputs.require("template")?, "template", 160)?),
        ),
    ]);
    crate::invoke(inputs, TEMPLATE_LIFECYCLE, args)
}

pub fn create_project(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let mut args = Map::from_iter([
        (
            "template".into(),
            json!(crate::text(inputs.require("template")?, "template", 160)?),
        ),
        (
            "projectName".into(),
            json!(crate::text(
                inputs.require("project-name")?,
                "project-name",
                200
            )?),
        ),
    ]);
    crate::optional_text(inputs, "project-id", "projectId", 160, &mut args)?;
    crate::invoke(inputs, CREATE_PROJECT, args)
}

pub fn render_list(data: &Value) -> String {
    format!(
        "{} templates matched; {} returned\n",
        data["total"].as_u64().unwrap_or(0),
        data["templates"].as_array().map(Vec::len).unwrap_or(0)
    )
}

pub fn render_template(data: &Value) -> String {
    let template = data.get("template").unwrap_or(data);
    format!(
        "{}  {} project forms\n",
        template["slug"].as_str().unwrap_or("template"),
        data["formTotal"]
            .as_u64()
            .or_else(|| template["formCount"].as_u64())
            .unwrap_or(0)
    )
}

pub fn render_mutation(data: &Value) -> String {
    format!(
        "{} {}\n",
        data["action"].as_str().unwrap_or("template"),
        data["template"]
            .as_str()
            .or_else(|| data["template_slug"].as_str())
            .unwrap_or("")
    )
}

pub fn render_project(data: &Value) -> String {
    format!(
        "project {} created from {}\n",
        data["project_id"].as_str().unwrap_or("?"),
        data["template_slug"].as_str().unwrap_or("?")
    )
}
