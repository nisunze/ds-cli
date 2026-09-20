//! `ds report project compute` — ask the cloud to compute the individual
//! reports of transformers and publish them to the project.
//!
//! This is the headless twin of the application's "Export reports" button.
//! Two doors already produced an individual report: `ds report project
//! export` runs the EDGE engine on this machine and seals the result into the
//! publication queue, and `ds report project combined` asks the cloud for the
//! COMBINED archive. Nothing headless asked the cloud for an individual, so
//! edge↔cloud parity — produce on the edge, produce in the cloud, watch them
//! meet in the project — could only be proven with a browser.
//!
//! The name is `compute`, not a second `export`: `export` is what the edge
//! engine does here, and the operation this asks for is the cloud's
//! computation. ds-brain owns everything after the request — write
//! governance, freshness, the claim, the fan-out to the cloud reporter and
//! the publication — and answers per transformer.

use ds_cli_auth::{ExportReportOutcome, ExportReportsReceipt, TransformerSet};
use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Authority, Chapter, Command, Effect, Example, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Value, json};

use super::{LANE_ARG, TRANSFORMER_ARG};

/// The report service refuses this action on its own terms: the write
/// governance lock is the one refusal `export_reports_only` alone emits, so
/// the shared `auth_input_invalid` text (grouping, layout) would misdirect.
const AUTH_INPUT: Refusal = Refusal {
    code: "auth_input_invalid",
    when: "the report service refused the request: a transformer another editor holds a lease on or that is permanently locked (HTTP 423 names the holder)",
    remedy: "wait for the lease or ask its holder; `ds design status` shows the lock",
};
const AUTH_REJECTED: Refusal = Refusal {
    code: "auth_rejected",
    when: "the fixed gateway rejects the verified request, the user lacks design.download or reports.export, or the project is archived or expired",
    remedy: "verify the account, its project access and capabilities, and the project lifecycle",
};
const AUTH_TRANSIENT: Refusal = Refusal {
    code: "auth_transient",
    when: "the report service or the cloud reporter is temporarily unavailable",
    remedy: "retry without changing local state; a report the cloud published stays published",
};
const AUTH_UNREADABLE: Refusal = Refusal {
    code: "auth_response_unreadable",
    when: "the receipt violates its closed contract: an unknown member, a row not asked for, or lists that disagree with the rows",
    remedy: "retry once, then update ds if it persists",
};
/// The cloud answered for every transformer and at least one errored.
pub const COMPUTE_PARTIAL: Refusal = Refusal {
    code: "report_compute_partial",
    when: "at least one requested transformer errored; `detail.results` names each with its error",
    remedy: "fix each failed row's cause, then re-run with only those names",
};
/// Nothing to compute: the project's inventory holds no active transformer.
pub const COMPUTE_SCOPE_EMPTY: Refusal = Refusal {
    code: "report_compute_scope_empty",
    when: "--transformer was omitted and the project has no active saved transformer",
    remedy: "run `ds report project scope`; the design, not the report, is what is empty",
};

const REFUSALS: &[Refusal] = &[
    super::NATIVE_PROFILE,
    super::NATIVE_PROFILE_DIGEST,
    super::NATIVE_PROFILE_UNSAFE,
    super::HEADLESS_SIGNED_OUT,
    super::HEADLESS_NO_PROJECT,
    super::PROJECT_CONTEXT_STALE,
    super::NATIVE_STATE_UNSAFE,
    super::NATIVE_STATE_UNAVAILABLE,
    super::NATIVE_STATE_PROTECTION,
    super::NATIVE_STATE_ROOT,
    super::NATIVE_STATE_CONFLICT,
    super::NATIVE_CLEANUP,
    super::AUTH_CONTEXT_MISMATCH,
    AUTH_INPUT,
    AUTH_REJECTED,
    super::AUTH_REVOKED,
    super::AUTH_IDENTITY_MISMATCH,
    AUTH_TRANSIENT,
    AUTH_UNREADABLE,
    super::NOT_FOUND,
    super::INVALID_SCOPE,
    super::RESERVED_IDENTITY,
    super::CONFIRMATION_REQUIRED,
    COMPUTE_SCOPE_EMPTY,
    COMPUTE_PARTIAL,
];

pub static COMMAND: Command = Command {
    id: "report.project.compute",
    path: &["report", "project", "compute"],
    contract: 1,
    summary: "Compute individual reports in the cloud (needs --yes).",
    purpose: "\
After CLI confirmation, restores the native user and asks the governed report \
service to compute the named transformers' individual reports in the cloud \
and publish them to its audience-fenced selected project — what the \
application's \"Export reports\" button asks. Without --transformer every \
active saved transformer is named, from the inventory `ds report project \
scope` shows. The service owns write governance, freshness (a fresh report \
is skipped) and the cloud reporter fan-out; `ds report project export` is \
the edge twin. Blocks until the service answers (up to ten minutes).",
    chapter: Chapter::Reports,
    effect: Effect::ArtifactWrite,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[TRANSFORMER_ARG, LANE_ARG],
    output: "\
Lane and selected-project identity/status, the `scope` sent, `partial`, the \
`computed`, `skipped` and `failed` names, and `results`: one row per \
requested transformer in request order with `outcome` (`success`; `skipped` \
with `reason`; `error` with `error`).",
    examples: &[Example {
        command: "ds report project compute --transformer akagerero --lane canary --yes --output json",
        note: "`ds design status --transformer akagerero` then shows the cloud's report stamp.",
        runnable: false,
    }],
    refusals: REFUSALS,
    reference: Some("docs/reference/report.md"),
    search: &["recompute", "regenerate report", "export reports"],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let lane = inputs.require("lane")?;
    let requested = super::transformer_set(inputs)?;
    // The route needs exact names — an empty list there is not "every active"
    // — so the omitted form is resolved from the same inventory `scope` reads.
    let (mode, scope) = if requested.is_empty() {
        let inventory = ds_cli_auth::transformer_inventory(lane, &requested)?;
        let names = active_names(&super::scope_json(&requested, inventory.result()));
        if names.is_empty() {
            return Err(Failure::conflict(
                COMPUTE_SCOPE_EMPTY.code,
                "the project has no active saved transformer to compute",
            )
            .remedy(COMPUTE_SCOPE_EMPTY.remedy)
            .next("ds report project scope"));
        }
        let scope = TransformerSet::new(names).map_err(|error| {
            Failure::invalid(super::INVALID_SCOPE.code, error.to_string())
                .remedy(super::INVALID_SCOPE.remedy)
        })?;
        ("all_active", scope)
    } else {
        ("explicit", requested)
    };
    let headless = ds_cli_auth::export_reports(lane, &scope)?;
    let mut output = super::project_receipt(&headless);
    output["scope"] = json!({"mode": mode, "requested": scope.names()});
    output
        .as_object_mut()
        .expect("receipt is an object")
        .extend(
            receipt_json(headless.result())
                .as_object()
                .expect("receipt fields are an object")
                .clone(),
        );
    if let Some(failure) = partial_refusal(&output) {
        return Err(failure);
    }
    Ok(output)
}

/// The participating names of a `scope_json` plan, in inventory order.
fn active_names(scope: &Value) -> Vec<String> {
    scope["participating"]
        .as_array()
        .map(|names| {
            names
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

/// The cloud's receipt, in the CLI's own keys.
fn receipt_json(receipt: &ExportReportsReceipt) -> Value {
    let results: Vec<Value> = receipt
        .results()
        .iter()
        .map(|row| {
            let mut entry = json!({"name": row.name(), "outcome": row.outcome().token()});
            match row.outcome() {
                ExportReportOutcome::Skipped => entry["reason"] = json!(row.reason()),
                ExportReportOutcome::Error => entry["error"] = json!(row.error()),
                ExportReportOutcome::Success => {}
            }
            entry
        })
        .collect();
    json!({
        "partial": receipt.partial(),
        "computed": receipt.computed(),
        "skipped": receipt.skipped(),
        "failed": receipt.failed(),
        "results": results,
    })
}

/// A run in which any transformer errored does not exit zero: a script that
/// asked for N reports and got N-1 must not read success. The receipt travels
/// in `detail` so the computed and skipped names are not lost with it.
fn partial_refusal(output: &Value) -> Option<Failure> {
    let failed: Vec<&str> = output["failed"]
        .as_array()?
        .iter()
        .filter_map(Value::as_str)
        .collect();
    if failed.is_empty() {
        return None;
    }
    Some(
        Failure::conflict(
            COMPUTE_PARTIAL.code,
            format!(
                "the cloud did not compute {} of {} requested report(s): {}",
                failed.len(),
                output["results"].as_array().map_or(0, Vec::len),
                failed.join(", "),
            ),
        )
        .remedy(COMPUTE_PARTIAL.remedy)
        .next(format!(
            "ds report project compute {} --yes",
            failed
                .iter()
                .map(|name| format!("--transformer {name}"))
                .collect::<Vec<_>>()
                .join(" ")
        ))
        .detail(json!({
            "computed": output["computed"].clone(),
            "skipped": output["skipped"].clone(),
            "results": output["results"].clone(),
        })),
    )
}

pub fn render(data: &Value) -> String {
    let mut out = format!(
        "project {} ({}) · {} · {} · {} computed · {} skipped · {} failed\n",
        data["project"]["project_name"].as_str().unwrap_or("?"),
        data["project"]["ds_project"].as_str().unwrap_or("?"),
        data["lane"].as_str().unwrap_or("?"),
        if data["partial"].as_bool().unwrap_or(false) {
            "partial"
        } else {
            "success"
        },
        data["computed"].as_array().map_or(0, Vec::len),
        data["skipped"].as_array().map_or(0, Vec::len),
        data["failed"].as_array().map_or(0, Vec::len),
    );
    if let Some(rows) = data["results"].as_array() {
        for row in rows {
            out.push_str(&format!(
                "  {:<28} {:<8} {}\n",
                row["name"].as_str().unwrap_or("?"),
                row["outcome"].as_str().unwrap_or("?"),
                row["reason"]
                    .as_str()
                    .or_else(|| row["error"].as_str())
                    .unwrap_or(""),
            ));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One operation, one command, the three inputs and nothing else: the
    /// scope, the lane, and the framework's `--yes` for an artifact write.
    #[test]
    fn the_command_is_the_cloud_twin_of_export_with_three_inputs() {
        assert_eq!(COMMAND.id, "report.project.compute");
        assert_eq!(COMMAND.path, &["report", "project", "compute"]);
        assert_eq!(COMMAND.effect, Effect::ArtifactWrite);
        assert_eq!(COMMAND.authority, Authority::HeadlessProject);
        assert_eq!(COMMAND.requires, Requires::Server);
        let args: Vec<&str> = COMMAND.args.iter().map(|arg| arg.name).collect();
        assert_eq!(args, ["transformer", "lane"]);
        assert!(COMMAND.effect.needs_confirmation());
        assert!(COMMAND.summary.contains("cloud"), "{}", COMMAND.summary);
        assert!(
            COMMAND.purpose.contains("ds report project export"),
            "the edge twin is named: {}",
            COMMAND.purpose
        );
        // The refusals the route emits, not the Combined Report's grouping ones.
        let codes: Vec<&str> = COMMAND.refusals.iter().map(|r| r.code).collect();
        assert!(codes.contains(&"report_compute_partial"));
        assert!(codes.contains(&"report_compute_scope_empty"));
        assert!(codes.contains(&"confirmation_required"));
        assert!(!codes.contains(&"report_grouping_stale"));
        assert!(!codes.contains(&"report_no_individual_artifacts"));
    }

    /// The omitted scope is the plan's participating names, in order, and
    /// nothing excluded or project-level.
    #[test]
    fn the_omitted_scope_is_the_plan_s_participating_names() {
        let scope = json!({
            "participating": ["akagerero", "tx_b"],
            "excluded": [{"name": "tx_r", "state": "retired"}],
            "project_level": [{"name": "mv_data", "state": "active"}],
        });
        assert_eq!(active_names(&scope), ["akagerero", "tx_b"]);
        assert!(active_names(&json!({})).is_empty());
    }

    /// A failed row is a refusal that keeps the whole receipt and names the
    /// exact re-run; a run with only computed and skipped rows is a success.
    #[test]
    fn a_failed_row_refuses_and_names_the_rerun() {
        let output = json!({
            "computed": ["akagerero"],
            "skipped": ["tx_b"],
            "failed": ["tx_c", "tx_d"],
            "results": [
                {"name": "akagerero", "outcome": "success"},
                {"name": "tx_b", "outcome": "skipped", "reason": "fresh"},
                {"name": "tx_c", "outcome": "error", "error": "reporter unreachable"},
                {"name": "tx_d", "outcome": "error", "error": "no design"}
            ]
        });
        let failure = partial_refusal(&output).expect("a failed row refuses");
        assert_eq!(failure.code(), "report_compute_partial");
        assert!(
            failure.message().contains("2 of 4"),
            "{}",
            failure.message()
        );
        assert!(
            failure.message().contains("tx_c, tx_d"),
            "{}",
            failure.message()
        );
        assert!(
            failure.next_commands().contains(
                &"ds report project compute --transformer tx_c --transformer tx_d --yes"
                    .to_string()
            ),
            "{:?}",
            failure.next_commands()
        );
        let detail = failure.detail_value().expect("the receipt travels");
        assert_eq!(detail["computed"], json!(["akagerero"]));
        assert_eq!(detail["results"].as_array().map(Vec::len), Some(4));

        let clean = json!({"computed": ["akagerero"], "skipped": [], "failed": [], "results": []});
        assert!(partial_refusal(&clean).is_none());
    }

    /// The one line a reader sees first says what the cloud did, per row.
    #[test]
    fn the_render_counts_and_lists_rows() {
        let data = json!({
            "project": {"project_name": "IT Rwanda", "ds_project": "it_rwanda"},
            "lane": "canary", "partial": false,
            "computed": ["akagerero"], "skipped": ["tx_b"], "failed": [],
            "results": [
                {"name": "akagerero", "outcome": "success"},
                {"name": "tx_b", "outcome": "skipped", "reason": "fresh"}
            ]
        });
        let rendered = render(&data);
        assert!(
            rendered.contains("1 computed · 1 skipped · 0 failed"),
            "{rendered}"
        );
        assert!(rendered.contains("akagerero"), "{rendered}");
        assert!(rendered.contains("fresh"), "{rendered}");
    }
}
