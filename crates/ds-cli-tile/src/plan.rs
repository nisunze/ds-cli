//! `ds tile plan` — combine status and preflight without dispatching.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{Authority, Chapter, Command, Effect, Example, Execution};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Value, json};

use crate::{FORCE_ARG, LANE_ARG, TYPE_ARG};

pub static COMMAND: Command = Command {
    id: "tile.plan",
    path: &["tile", "plan"],
    contract: 1,
    summary: "Decide whether a run is needed and preflight it, without running.",
    purpose: "\
Restores the native user and reads the fixed status for its audience-fenced \
selected project. It applies the Pipeline staleness rule — never built, dirty \
or --force means a run; current and clean means no run — then calls the fixed \
preflight only when work would dispatch. It verifies both reads name the same \
selected project and never calls generation. No project, Desktop descriptor, \
URL, body or action override is accepted.",
    chapter: Chapter::VectorTiles,
    effect: Effect::LocalAuthState,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[TYPE_ARG, FORCE_ARG, LANE_ARG],
    output: "\
Lane and selected-project identity/status, `type`, `force`, `dispatched: \
false`, `wouldDispatch`, `reason`, the status used for the decision, and \
`preflight` (null when no run is needed).",
    examples: &[Example {
        command: "ds tile plan --type design --force --output json",
        note: "After a restyle or a vocabulary change the output is not dirty; --force is the honest flag.",
        runnable: false,
    }],
    refusals: crate::NATIVE_PLAN_REFUSALS,
    reference: Some("docs/reference/tile.md"),
    availability: ds_cli_auth::native_availability,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let lane = inputs.require("lane")?;
    let kind = crate::tile_type(inputs.require("type")?);
    let force = inputs.switch("force");
    let status = ds_cli_auth::tile_status(lane, kind)?;
    let project = crate::operation_project(&status);
    let result = status.result();
    let (would_dispatch, reason) =
        crate::plan_decision(result.status(), result.tiled_at(), result.dirty(), force);
    let preflight = if would_dispatch {
        let preflight = ds_cli_auth::tile_preflight(lane, kind)?;
        crate::require_same_project(&project, &crate::preflight_project(&preflight))?;
        crate::preflight_json(preflight.result())
    } else {
        Value::Null
    };
    let mut output = project;
    output
        .as_object_mut()
        .expect("receipt is an object")
        .extend(
            json!({
                "type": kind.token(),
                "force": force,
                "dispatched": false,
                "wouldDispatch": would_dispatch,
                "reason": reason,
                "status": crate::operation_json(result),
                "preflight": preflight,
            })
            .as_object()
            .expect("static plan projection is an object")
            .clone(),
        );
    Ok(output)
}

pub fn render(data: &Value) -> String {
    render_decision(data)
}

pub fn render_decision(data: &Value) -> String {
    let kind = data["type"].as_str().unwrap_or("?");
    let verdict = match (
        data["dispatched"].as_bool().unwrap_or(false),
        data["wouldDispatch"].as_bool().unwrap_or(false),
    ) {
        (true, _) => "run started",
        (false, true) => "a run is needed (not started)",
        (false, false) => "no run needed",
    };
    let mut out = format!("{kind} tiles · {verdict}\n");
    if let Some(reason) = data["reason"].as_str()
        && !reason.is_empty()
    {
        out.push_str(&format!("  {}\n", crate::truncate(reason, 110)));
    }
    if !data["preflight"].is_null() {
        out.push_str(&format!(
            "  preflight: {}\n",
            crate::preflight_line(&data["preflight"])
        ));
    }
    if let Some(message) = data["result"]["message"].as_str()
        && !message.is_empty()
    {
        out.push_str(&format!("  {}\n", crate::truncate(message, 110)));
    }
    out
}
