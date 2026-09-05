//! `ds map layer reorder` — save canonical project-layer order overrides.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, ArgKind, Authority, Chapter, Command, Effect, Example, Execution,
};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Value, json};

use super::native::LANE_ARG;

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
    contract: 2,
    summary: "Save canonical project-layer order overrides (needs --yes).",
    purpose: "Validates every id against a fresh-enough assembled project layer response, then saves explicit order overrides through ds-brain. Runtime MapLibre ids are refused. Geometry and platform stack safety still govern final draw grouping.",
    chapter: Chapter::Survey,
    effect: Effect::GlobalWrite,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[ORDER_ARG, LANE_ARG],
    output: "Project, the exact reviewed id/order pairs, and applied/persisted flags.",
    examples: &[Example {
        command: "ds map layer reorder --order gt/roads=120 --order gt/schools=220 --yes --output json",
        note: "Take ids from `ds map layer list`; never construct them.",
        runnable: false,
    }],
    refusals: super::native::NATIVE_WRITE_REFUSALS,
    reference: Some("docs/reference/map.md"),
    availability: ds_cli_auth::native_availability,
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
    let request: Vec<ds_cli_auth::LayerOrder> = orders
        .iter()
        .map(|row| ds_cli_auth::LayerOrder {
            layer_id: row["layerId"].as_str().expect("parsed id").to_owned(),
            order: row["order"].as_i64().expect("parsed order"),
        })
        .collect();
    let result = ds_cli_auth::layer_reorder(inputs.require("lane")?, &request)?;
    Ok(
        json!({"lane": result.lane(), "project": result.project_id(), "orders": orders, "applied": true, "persisted": true}),
    )
}

pub fn render(data: &Value) -> String {
    format!(
        "saved {} layer-order overrides for {}\n",
        data["orders"].as_array().map_or(0, Vec::len),
        data["project"].as_str().unwrap_or("?"),
    )
}
