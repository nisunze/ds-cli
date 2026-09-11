//! `ds map local list` — what this machine's prepared local layer catalogue holds.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{Arg, Authority, Chapter, Command, Effect, Example, Execution};
use ds_cli_contract::{Context, Inputs};
use ds_layer_store::prepared::{self, Op};
use serde_json::{Value, json};

pub static COMMAND: Command = Command {
    id: "map.local.list",
    path: &["map", "local", "list"],
    contract: 1,
    summary: "List this machine's prepared local layers.",
    purpose: "Reads the prepared-layer catalogue this lane and DS account keep on this host, and answers the bounded rows the shared kernel projects: id, name, source, geometry, feature count, visibility and where each layer came from. Never the style, never the schema, never a folder handle — a listing is not a capability. Machine-local only: these are not project layers (`ds map layer list`) and a remote Server never reads this host's catalogue. No sign-in and no open map.",
    chapter: Chapter::Survey,
    effect: Effect::ReadOnly,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[
        Arg::value("limit", "<n>", "Report at most this many layers; 1..2000.").default("100"),
        super::LANE_ARG,
        super::ACCOUNT_ARG,
    ],
    output: "Lane, account, layer count, and one row per layer with its id, name, source, geometry type, feature count, visibility, creation time and origin.",
    examples: &[Example {
        command: "ds map local list --output json",
        note: "Reads a file; an empty store answers zero layers and creates nothing.",
        runnable: false,
    }],
    refusals: &[
        super::STORE_REFUSED,
        super::MALFORMED_DESCRIPTOR,
        super::DUPLICATE_LAYER,
        super::SCOPE_MISMATCH,
        super::UNSUPPORTED_SOURCE_KIND,
        crate::INVALID_NUMBER,
    ],
    reference: Some("docs/reference/map.md"),
    availability: super::availability,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let limit = crate::integer(inputs.require("limit")?, "limit", 1, 2_000)? as usize;
    let scope = super::scope(inputs)?;
    let answer =
        prepared::execute(&scope, Op::List { limit: Some(limit) }).map_err(super::refuse)?;
    let receipt = &answer["receipt"];
    let rows: Vec<Value> = receipt["layers"]
        .as_array()
        .into_iter()
        .flatten()
        .map(super::row)
        .collect();
    let count = receipt["count"].as_u64().unwrap_or(rows.len() as u64);
    Ok(super::stamped(
        &scope,
        &answer,
        json!({
            "layer_count": count,
            "layers": rows,
            "more": count > rows.len() as u64,
        }),
    ))
}

pub fn render(data: &Value) -> String {
    let mut out = format!(
        "{} prepared local layers · {}/{}\n",
        data["layer_count"],
        data["lane"].as_str().unwrap_or("?"),
        data["account"].as_str().unwrap_or("?"),
    );
    for row in data["layers"].as_array().into_iter().flatten() {
        out.push_str(&super::render_row(row));
    }
    if data["more"].as_bool().unwrap_or(false) {
        out.push_str("more layers than the limit shown\n");
    }
    out
}
