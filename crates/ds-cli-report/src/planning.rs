//! Native print inventory and pure report planning; IO stays in the host.
use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Availability, Chapter, Command, Effect, Example, Execution, Refusal,
};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Value, json};
use std::io::Read;

const INVALID: Refusal = Refusal {
    code: "report_plan_invalid",
    when: "The limit or bounded report planning request is invalid",
    remedy: "Correct the reported field; see docs/reference/report.md",
};
const REFUSALS: &[Refusal] = &[
    INVALID,
    crate::project::NATIVE_PROFILE,
    crate::project::NATIVE_PROFILE_DIGEST,
    crate::project::NATIVE_PROFILE_UNSAFE,
    crate::project::HEADLESS_SIGNED_OUT,
    crate::project::HEADLESS_NO_PROJECT,
    crate::project::PROJECT_CONTEXT_STALE,
];
fn invalid(error: impl std::fmt::Display) -> Failure {
    Failure::invalid(INVALID.code, error.to_string()).remedy(INVALID.remedy)
}
fn available() -> Availability {
    Availability::Available
}

pub static TRANSFORMERS: Command = Command {
    id: "report.transformers",
    path: &["report", "transformers"],
    contract: 1,
    summary: "List printable transformers from the selected project, headlessly.",
    purpose: "Reads the selected project's fresh status rows through the native user and asks the printing kernel for the bounded printable inventory. No browser, map, or project override. Local browser rooms are not inspected: cached and dirty are null, with local_rooms_known=false. The paired desktop inventory uses the same kernel with its own room facts.",
    chapter: Chapter::Reports,
    effect: Effect::LocalAuthState,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[
        crate::project::LANE_ARG,
        Arg::value("limit", "<n>", "Maximum printable rows, 1..500.").default("100"),
    ],
    output: "Lane and selected project, total, transformers [{name,kind,server_version,cached,dirty}], local_rooms_known, and more.omitted.",
    examples: &[Example {
        command: "ds report transformers --limit 20 --output json",
        note: "The selected project's printable rows; local cache state is unknown.",
        runnable: false,
    }],
    refusals: REFUSALS,
    reference: Some("docs/reference/report.md"),
    availability: ds_cli_auth::native_availability,
};
pub fn transformers(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
    let limit = inputs.require("limit")?.parse::<usize>().map_err(invalid)?;
    if !(1..=500).contains(&limit) {
        return Err(invalid("limit must be from 1 through 500"));
    }
    let requested = ds_cli_auth::TransformerSet::new(Vec::<String>::new()).map_err(invalid)?;
    let headless = ds_cli_auth::transformer_status(inputs.require("lane")?, &requested)?;
    let rows: Vec<Value> = headless
        .result()
        .rows()
        .iter()
        .map(|row| row.row().clone())
        .collect();
    let mut output = inventory(&rows, limit)?;
    output["lane"] = json!(headless.lane());
    output["project"] = json!({"ds_project":headless.project_id(),"project_name":headless.project_name(),"status":headless.project_status()});
    Ok(output)
}
fn inventory(rows: &[Value], limit: usize) -> Result<Value, Failure> {
    let request = json!({"op":"printable_transformers","rows":rows,"limit":limit});
    let reply =
        ds_command_kernel::printing::evaluate(&serde_json::to_vec(&request).map_err(invalid)?)
            .map_err(invalid)?;
    serde_json::from_str(&reply).map_err(invalid)
}

pub static PLAN: Command = Command {
    id: "report.plan",
    path: &["report", "plan"],
    contract: 1,
    summary: "Decide an export request or fold a batch's outcomes without IO.",
    purpose: "Calls the same pure report kernel as the GUI. Export planning receives project, transformer, optional scope/selection and the host's active_project fact; batch-outcome receives results. This command performs no export and grants no project access. Request files are limited to 4 MiB; the action is fixed by --action, not supplied inside the document.",
    chapter: Chapter::Reports,
    effect: Effect::ReadOnly,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[
        Arg::value("action", "<action>", "Planning operation.")
            .choices(&["export", "batch-outcome"])
            .required(),
        Arg::value(
            "request",
            "<json-file>",
            "Request fields, without a command key; at most 4 MiB.",
        )
        .required(),
    ],
    output: "Export: project, target, kind, scope, label, selection and preparation policy. Batch: ordered results and failed count.",
    examples: &[Example {
        command: "ds report plan --action export --request export.json --output json",
        note: "Inspect a report decision without running an exporter.",
        runnable: false,
    }],
    refusals: &[INVALID],
    reference: Some("docs/reference/report.md"),
    availability: available,
};
pub fn plan(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
    const MAX: usize = 4 * 1024 * 1024;
    let mut bytes = Vec::new();
    std::fs::File::open(inputs.require("request")?)
        .map_err(invalid)?
        .take((MAX + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(invalid)?;
    if bytes.len() > MAX {
        return Err(invalid("Report planning request exceeds 4 MiB"));
    }
    let mut request: Value = serde_json::from_slice(&bytes).map_err(invalid)?;
    let fields = request
        .as_object_mut()
        .ok_or_else(|| invalid("Expected a request object"))?;
    if fields.contains_key("command") {
        return Err(invalid("Use --action; a command key is not accepted"));
    }
    let command = match inputs.require("action")? {
        "export" => "export",
        "batch-outcome" => "batch_outcome",
        _ => return Err(invalid("Unknown report planning action")),
    };
    fields.insert("command".into(), json!(command));
    let reply =
        ds_command_kernel::report::evaluate(&serde_json::to_vec(&request).map_err(invalid)?)
            .map_err(invalid)?;
    let mut reply: Value = serde_json::from_str(&reply).map_err(invalid)?;
    Ok(reply["result"].take())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_rows_use_kernel_kind_and_do_not_invent_local_cache_facts() {
        let rows = vec![
            json!({"name":"tx_b"}),
            json!({"name":"combined_transformer"}),
            json!({"name":"tx_a","metadata":{"version":"4"}}),
            json!({"name":"mv_data"}),
            json!({"name":"collisions"}),
        ];
        let out = inventory(&rows, 1).unwrap();
        assert_eq!(out["total"], 2);
        assert_eq!(out["more"]["omitted"], 1);
        assert_eq!(out["local_rooms_known"], false);
        assert_eq!(out["transformers"][0]["name"], "tx_a");
        assert_eq!(out["transformers"][0]["server_version"], 4.0);
        assert!(out["transformers"][0]["cached"].is_null());
        assert!(out["transformers"][0]["dirty"].is_null());
    }
}
