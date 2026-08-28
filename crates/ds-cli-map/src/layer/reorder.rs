//! `ds map layer reorder` — save canonical project-layer order overrides.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, ArgKind, Authority, Chapter, Command, Effect, Example, Execution, Refusal,
};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Value, json};

use crate::DESCRIPTOR_ARG;

const ORDER_ARG: Arg = Arg {
    name: "order",
    kind: ArgKind::Repeated,
    value: "<config-id=integer>",
    required: true,
    default: None,
    choices: &[],
    summary: "Canonical id and desired order. Repeat for each override.",
};

pub static COMMAND: Command = Command {
    id: "map.layer.reorder",
    path: &["map", "layer", "reorder"],
    contract: 1,
    summary: "Save canonical project-layer order overrides (needs --yes).",
    purpose: "Validates every id against a fresh-enough assembled project layer response, then saves explicit order overrides through ds-brain. Runtime MapLibre ids are refused. Geometry and platform stack safety still govern final draw grouping.",
    chapter: Chapter::Survey,
    effect: Effect::GlobalWrite,
    authority: Authority::Project,
    execution: Execution::Sync,
    args: &[ORDER_ARG, DESCRIPTOR_ARG],
    output: "Project, the exact reviewed id/order pairs, and applied/persisted flags.",
    examples: &[Example {
        command: "ds map layer reorder --order gt/roads=120 --order gt/schools=220 --yes --output json",
        note: "Take ids from `ds map layer list`; never construct them.",
        runnable: false,
    }],
    refusals: &[
        crate::NOT_PAIRED,
        crate::AMBIGUOUS,
        crate::UNREACHABLE,
        crate::PAIRING_REJECTED,
        crate::REFUSED,
        crate::UNSUPPORTED,
        crate::UNREADABLE,
        crate::SIGNED_OUT,
        Refusal {
            code: "confirmation_required",
            when: "--yes was not supplied",
            remedy: "review `ds map layer list`, then repeat the exact command with --yes",
        },
        Refusal {
            code: "invalid_order",
            when: "an item is not canonical-id=integer or repeats an id",
            remedy: "copy ids from `ds map layer list --output json`",
        },
    ],
    reference: Some("docs/reference/map.md"),
    availability: crate::paired_availability,
};

fn parse_orders(values: &[String]) -> Result<Vec<Value>, Failure> {
    let mut seen = std::collections::BTreeSet::new();
    values
        .iter()
        .map(|value| {
            let Some((id, order)) = value.rsplit_once('=') else {
                return Err(
                    Failure::invalid("invalid_order", "--order must be config-id=integer")
                        .remedy("copy the id from `ds map layer list --output json`"),
                );
            };
            let id = id.trim();
            if id.is_empty() || !seen.insert(id.to_string()) {
                return Err(Failure::invalid(
                    "invalid_order",
                    "layer ids must be non-empty and unique",
                )
                .remedy("pass each canonical id once"));
            }
            let order = order.trim().parse::<i64>().map_err(|_| {
                Failure::invalid(
                    "invalid_order",
                    format!("{value} does not end in an integer"),
                )
                .remedy("use config-id=integer")
            })?;
            if !(-1_000_000..=1_000_000).contains(&order) {
                return Err(Failure::invalid(
                    "invalid_order",
                    "order is outside -1000000..1000000",
                )
                .remedy("use a bounded integer"));
            }
            Ok(json!({ "layerId": id, "order": order }))
        })
        .collect()
}

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let orders = parse_orders(inputs.repeated("order"))?;
    let descriptor = crate::paired(inputs.value("desktop-descriptor"))?;
    crate::invoke(
        &descriptor,
        &crate::LAYERS_REORDER,
        json!({ "orders": orders, "apply": true }),
        crate::API_TIMEOUT,
    )
    .map_err(super::classify)
}

pub fn render(data: &Value) -> String {
    format!(
        "saved {} layer-order overrides for {} · map updated: {}\n",
        data["orders"].as_array().map_or(0, Vec::len),
        data["project"].as_str().unwrap_or("?"),
        data["mapUpdated"].as_bool().unwrap_or(false),
    )
}
