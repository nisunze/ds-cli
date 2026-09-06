//! Bounded catalog edits owned by the authenticated native configuration client.
use ds_cli_contract::{
    Context, Inputs,
    outcome::Failure,
    spec::{Arg, Authority, Chapter, Command, Effect, Execution},
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
    purpose: "Reads the selected project's customer or meter catalog without Desktop. Inspect canonical names, aliases and demand metadata before changing data or code. Results are paged; no configuration is modified.",
    args: &[
        LANE,
        Arg::value("kind", "<customer|meter>", "Catalog to inspect.")
            .required()
            .choices(&["customer", "meter"]),
        Arg::value("offset", "<n>", "Zero-based first catalog row.").default("0"),
        Arg::value("limit", "<n>", "Maximum rows, between 1 and 100.").default("20"),
    ],
    output: "Project, catalog kind, rows, total, offset and omitted row count.",
    examples: &[],
    refusals: super::feeder_limits::REFUSALS,
    reference: Some("docs/reference/design.md"),
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
    let receipt = ds_cli_auth::feeder_configuration(inputs.require("lane")?, None)?;
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
        json!({"project":receipt.summary["project"],"kind":kind,"rows":returned,"total":rows.len(),"offset":offset,"more":rows.len().saturating_sub(offset.saturating_add(returned.len()))}),
    )
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
    args: &[LANE, NAME],
    output: "Project, saved state and bounded meter categories.",
    examples: &[],
    refusals: super::feeder_limits::REFUSALS,
    reference: Some("docs/reference/design.md"),
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
    args: &[LANE, ALIAS, CATEGORY],
    output: "Project, saved state, alias and canonical category.",
    examples: &[],
    refusals: super::feeder_limits::REFUSALS,
    reference: Some("docs/reference/design.md"),
    availability: ds_cli_auth::native_availability,
};
pub fn meter(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
    let receipt = ds_cli_auth::ensure_meter_type(inputs.require("lane")?, inputs.require("name")?)?;
    Ok(
        json!({"project":receipt.summary["project"],"saved":receipt.summary["saved"],"meter_types":receipt.summary["meter_types"]}),
    )
}
pub fn alias(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
    let alias = inputs.require("alias")?;
    let category = inputs.require("category")?;
    let receipt = ds_cli_auth::customer_category_alias(inputs.require("lane")?, alias, category)?;
    Ok(
        json!({"project":receipt.summary["project"],"saved":receipt.summary["saved"],"alias":alias,"category":category}),
    )
}
pub fn render(value: &Value) -> String {
    format!("{}\n", value)
}
