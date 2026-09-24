//! Bounded catalog edits owned by the authenticated native configuration client.
use ds_cli_contract::{
    Context, Inputs,
    outcome::Failure,
    spec::{Arg, Authority, Chapter, Command, Effect, Execution, Requires},
};
use serde_json::{Value, json};

const LANE: Arg = Arg::value("lane", "<stable|canary>", "Deployment lane.")
    .default("stable")
    .choices(&["stable", "canary"]);
const NAME: Arg = Arg::value(
    "name",
    "<name>",
    "Distinct meter type to retain in the project catalog.",
)
.required();
const ALIAS: Arg = Arg::value(
    "alias",
    "<name>",
    "Customer source label to map through the catalog.",
)
.required();
const CATEGORY: Arg = Arg::value(
    "category",
    "<name>",
    "Existing canonical customer category; its load settings are preserved.",
)
.required();

pub static READ: Command = Command {
    id: "design.categories.read",
    path: &["design", "categories", "read"],
    contract: 1,
    chapter: Chapter::Design,
    authority: Authority::HeadlessProject,
    effect: Effect::ReadOnly,
    execution: Execution::Sync,
    summary: "Inspect fresh category seeds and aliases behind Dirty Categories.",
    purpose: "Reads the named project's customer or meter catalog without Desktop. Inspect canonical names, aliases and demand metadata before changing data or code. Results are paged; no configuration is modified.",
    args: &[
        crate::PROJECT_ARG,
        LANE,
        Arg::value("kind", "<customer|meter>", "Catalog to inspect.")
            .required()
            .choices(&["customer", "meter"]),
        Arg::value("offset", "<n>", "Zero-based first catalog row.").default("0"),
        Arg::value("limit", "<n>", "Maximum rows, between 1 and 100.").default("20"),
    ],
    output: "Project, catalog kind, rows, total, offset, omitted row count, and the hazards this catalog imposes on a report: which category the fallback is and what decided it, the source labels two categories claim, unnamed rows, and the client/country scopes present.",
    examples: &[],
    refusals: super::feeder_limits::REFUSALS,
    reference: Some("docs/reference/design.md"),
    search: &[],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};

pub fn read(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
    let invalid = || {
        Failure::invalid(
            "invalid_catalog_page",
            "offset must be a nonnegative integer and limit between 1 and 100",
        )
    };
    let offset = inputs
        .require("offset")?
        .parse::<usize>()
        .map_err(|_| invalid())?;
    let limit = inputs
        .require("limit")?
        .parse::<usize>()
        .ok()
        .filter(|n| (1..=100).contains(n))
        .ok_or_else(invalid)?;
    let kind = inputs.require("kind")?;
    let sheet = if kind == "customer" {
        "cust_category"
    } else {
        "cust_meter_type"
    };
    let receipt = ds_cli_auth::feeder_configuration(
        inputs.require("lane")?,
        inputs.require("project")?,
        None,
    )?;
    let rows = receipt.document["sheets"][sheet]
        .as_array()
        .ok_or_else(|| {
            Failure::unavailable(
                "auth_response_unreadable",
                "the selected category catalog is missing",
            )
        })?;
    let returned = rows.iter().skip(offset).take(limit).collect::<Vec<_>>();
    Ok(
        json!({"project":receipt.summary["project"],"kind":kind,"rows":returned,"total":rows.len(),"offset":offset,"more":rows.len().saturating_sub(offset.saturating_add(returned.len())),
            "hazards":hazards(&receipt.document)}),
    )
}

/// What this catalog will do to a report, read off the same document the rows
/// came from. Paging shows the seed; this shows the consequences a page cannot:
/// which row decides the fallback, which labels two categories claim, and how
/// many rows carry no canonical name at all.
fn hazards(document: &Value) -> Value {
    ds_command_kernel::design_config::catalog_hazards(&document["sheets"])
}

pub static METER: Command = Command {
    id: "design.meter-types.ensure",
    path: &["design", "meter-types", "ensure"],
    contract: 1,
    chapter: Chapter::Design,
    authority: Authority::HeadlessProject,
    effect: Effect::GlobalWrite,
    execution: Execution::Sync,
    summary: "Ensure a distinct project meter category exists.",
    purpose: "Adds one missing canonical meter type to fresh project configuration, preserves existing rows, and verifies the saved catalog. Requires no Desktop and changes no customer records.",
    args: &[crate::PROJECT_ARG, LANE, NAME],
    output: "Project, saved state and bounded meter categories.",
    examples: &[],
    refusals: super::feeder_limits::REFUSALS,
    reference: Some("docs/reference/design.md"),
    search: &[],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};
pub static ALIAS_SET: Command = Command {
    id: "design.customer-categories.alias",
    path: &["design", "customer-categories", "alias"],
    contract: 1,
    chapter: Chapter::Design,
    authority: Authority::HeadlessProject,
    effect: Effect::GlobalWrite,
    execution: Execution::Sync,
    summary: "Map a customer source label to an existing catalog category.",
    purpose: "Adds one alias to an existing customer category in fresh configuration and verifies the saved catalog. Preserves demand settings and source records; refuses aliases already owned by a different category.",
    args: &[crate::PROJECT_ARG, LANE, ALIAS, CATEGORY],
    output: "Project, saved state, alias and canonical category.",
    examples: &[],
    refusals: super::feeder_limits::REFUSALS,
    reference: Some("docs/reference/design.md"),
    search: &[],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};
pub fn meter(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
    let receipt = ds_cli_auth::ensure_meter_type(
        inputs.require("lane")?,
        inputs.require("project")?,
        inputs.require("name")?,
    )?;
    Ok(
        json!({"project":receipt.summary["project"],"saved":receipt.summary["saved"],"meter_types":receipt.summary["meter_types"]}),
    )
}
pub fn alias(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
    let alias = inputs.require("alias")?;
    let category = inputs.require("category")?;
    let receipt = ds_cli_auth::customer_category_alias(
        inputs.require("lane")?,
        inputs.require("project")?,
        alias,
        category,
    )?;
    Ok(
        json!({"project":receipt.summary["project"],"saved":receipt.summary["saved"],"alias":alias,"category":category}),
    )
}
const RETIRE_NAME: Arg = Arg::value(
    "name",
    "<name>",
    "Canonical customer category to drop from this project's catalog.",
)
.required();

pub static CUSTOMER_RETIRE: Command = Command {
    id: "design.customer-categories.retire",
    path: &["design", "customer-categories", "retire"],
    contract: 1,
    chapter: Chapter::Design,
    authority: Authority::HeadlessProject,
    effect: Effect::GlobalWrite,
    execution: Execution::Sync,
    summary: "Drop one canonical customer category from the project catalog.",
    purpose: "Removes a category a project should never have been seeded with — a second client's vocabulary, a duplicate — so reporting stops offering and validating against it. Requires no Desktop and changes no customer records; source labels still spelled that way become dirty categories the next report names. Refuses while the category is the report fallback purely because it is first in the catalog, because retiring it would move every unrecognised customer somewhere new.",
    args: &[crate::PROJECT_ARG, LANE, RETIRE_NAME],
    output: "Project, saved state and the catalog hazards after the edit.",
    examples: &[],
    refusals: super::feeder_limits::REFUSALS,
    reference: Some("docs/reference/design.md"),
    search: &[],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};

pub static CUSTOMER_RETIRE_UNNAMED: Command = Command {
    id: "design.customer-categories.retire-unnamed",
    path: &["design", "customer-categories", "retire-unnamed"],
    contract: 1,
    chapter: Chapter::Design,
    authority: Authority::HeadlessProject,
    effect: Effect::GlobalWrite,
    execution: Execution::Sync,
    summary: "Drop catalog rows that carry no canonical customer category.",
    purpose: "Removes the blank rows a seeding pass left behind. A row with no canonical name groups nothing, validates nothing and can never be the fallback, but it is still offered wherever the catalog is listed. Requires no Desktop and changes no customer records; refuses when every row already carries a name.",
    args: &[crate::PROJECT_ARG, LANE],
    output: "Project, saved state and the catalog hazards after the edit.",
    examples: &[],
    refusals: super::feeder_limits::REFUSALS,
    reference: Some("docs/reference/design.md"),
    search: &[],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};

pub static CUSTOMER_RENAME: Command = Command {
    id: "design.customer-categories.rename",
    path: &["design", "customer-categories", "rename"],
    contract: 1,
    chapter: Chapter::Design,
    authority: Authority::HeadlessProject,
    effect: Effect::GlobalWrite,
    execution: Execution::Sync,
    summary: "Give one customer category a different canonical name.",
    purpose: "Renames the name reports group and total under, keeping the category's demand settings and every source label that already resolves to it — including the old name, which stored customer records still carry. Requires no Desktop. Refuses a new name another category already claims as its own name or as one of its aliases, so a rename cannot manufacture a contested label.",
    args: &[
        crate::PROJECT_ARG,
        LANE,
        Arg::value("from", "<name>", "Canonical category to rename.").required(),
        Arg::value("to", "<name>", "New canonical name for that category.").required(),
    ],
    output: "Project, saved state and the catalog hazards after the edit.",
    examples: &[],
    refusals: super::feeder_limits::REFUSALS,
    reference: Some("docs/reference/design.md"),
    search: &[],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};

pub static CUSTOMER_UNBIND: Command = Command {
    id: "design.customer-categories.unbind",
    path: &["design", "customer-categories", "unbind"],
    contract: 1,
    chapter: Chapter::Design,
    authority: Authority::HeadlessProject,
    effect: Effect::GlobalWrite,
    execution: Execution::Sync,
    summary: "Take one source label away from one customer category.",
    purpose: "Resolves a label two categories claim. Reporting drops a contested label from its alias map entirely, so the customers spelled that way stop resolving and are counted under the fallback instead. Naming the category that loses the label is the decision; `design customer-categories alias` then binds it to the intended owner. Requires no Desktop and changes no customer records.",
    args: &[
        crate::PROJECT_ARG,
        LANE,
        Arg::value("alias", "<name>", "Source label to unbind.").required(),
        Arg::value(
            "category",
            "<name>",
            "Canonical category that should stop claiming that label.",
        )
        .required(),
    ],
    output: "Project, saved state and the catalog hazards after the edit.",
    examples: &[],
    refusals: super::feeder_limits::REFUSALS,
    reference: Some("docs/reference/design.md"),
    search: &[],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};

pub static METER_DEFAULT: Command = Command {
    id: "design.meter-types.default",
    path: &["design", "meter-types", "default"],
    contract: 1,
    chapter: Chapter::Design,
    authority: Authority::HeadlessProject,
    effect: Effect::GlobalWrite,
    execution: Execution::Sync,
    summary: "Choose which meter type reporting falls back to.",
    purpose: "Reporting has no governed setting for the phase-type fallback: it takes the first named row of the meter catalog, so a meter reading the catalog does not recognise is counted as whichever type happened to be seeded first. This moves a named type to the front, turning that ordering accident into a stated choice. Requires no Desktop, seeds nothing and changes no customer records; use `design meter-types ensure` to add a type that is missing.",
    args: &[crate::PROJECT_ARG, LANE, NAME],
    output: "Project, saved state, ordered meter types and the catalog hazards after the edit.",
    examples: &[],
    refusals: super::feeder_limits::REFUSALS,
    reference: Some("docs/reference/design.md"),
    search: &[],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};

fn housekeeping(
    inputs: &Inputs,
    change: ds_client_core::ProjectConfigurationChange,
) -> Result<Value, Failure> {
    let receipt = ds_cli_auth::catalog_housekeeping(
        inputs.require("lane")?,
        inputs.require("project")?,
        change,
    )?;
    Ok(
        json!({"project":receipt.summary["project"],"saved":receipt.summary["saved"],"hazards":hazards(&receipt.document)}),
    )
}

pub fn customer_retire(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
    housekeeping(
        inputs,
        ds_client_core::ProjectConfigurationChange::RetireCustomerCategory {
            name: Some(inputs.require("name")?.to_owned()),
        },
    )
}

pub fn customer_retire_unnamed(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
    housekeeping(
        inputs,
        ds_client_core::ProjectConfigurationChange::RetireCustomerCategory { name: None },
    )
}

pub fn customer_rename(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
    housekeeping(
        inputs,
        ds_client_core::ProjectConfigurationChange::RenameCustomerCategory {
            from: inputs.require("from")?.to_owned(),
            to: inputs.require("to")?.to_owned(),
        },
    )
}

pub fn customer_unbind(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
    housekeeping(
        inputs,
        ds_client_core::ProjectConfigurationChange::UnbindCustomerAlias {
            alias: inputs.require("alias")?.to_owned(),
            category: inputs.require("category")?.to_owned(),
        },
    )
}

pub fn meter_default(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
    let receipt = ds_cli_auth::catalog_housekeeping(
        inputs.require("lane")?,
        inputs.require("project")?,
        ds_client_core::ProjectConfigurationChange::DefaultMeterType {
            name: inputs.require("name")?.to_owned(),
        },
    )?;
    Ok(
        json!({"project":receipt.summary["project"],"saved":receipt.summary["saved"],"meter_types":receipt.summary["meter_types"],"hazards":hazards(&receipt.document)}),
    )
}

pub fn render(value: &Value) -> String {
    format!("{}\n", value)
}
