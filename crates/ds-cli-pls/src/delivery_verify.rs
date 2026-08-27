//! `ds pls delivery-verify` — one bounded native delivery receipt.

use ds_cli_contract::spec::{
    Arg, Authority, Availability, Chapter, Command, Effect, Example, Execution, Refusal,
};
use ds_cli_contract::{Context, Failure, Inputs};
use ds_grid_tasks::{VerifyPlsDeliveryRequest, verify_pls_delivery};
use serde_json::{Value, json};

use crate::{source_path, workspace_path};

pub static COMMAND: Command = Command {
    id: "pls.delivery-verify",
    path: &["pls", "delivery-verify"],
    contract: 1,
    summary: "Verify one terrain and label delivery against its baseline.",
    purpose: "Reads an untouched baseline, the delivered closed workspace, and the exact terrain point batch. One receipt proves terrain counts and elevation deltas, unchanged baseline terrain/NUM/DON prefixes, attachment closure, and native phase/OPGW support-chain readback. It writes nothing and refuses instead of repairing a failed delivery.",
    chapter: Chapter::PlsCadd,
    effect: Effect::Discovery,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[
        Arg::value("baseline", "<dir>", "Untouched baseline workspace.").required(),
        Arg::value("workspace", "<dir>", "Closed delivered workspace.").required(),
        Arg::value(
            "points",
            "<json>",
            "The exact point batch supplied to terrain reconciliation.",
        )
        .required(),
    ],
    output: "One verification receipt with workspace and native member digests, terrain counts/delta distribution, prefix verdicts, attachment closure, phase/OPGW section and complete support-chain counts, and the remaining engineer decision.",
    examples: &[Example {
        command: "ds pls delivery-verify --baseline ./baseline --workspace ./labelled --points ./points.json --output json",
        note: "Read back every delivery invariant in one non-writing receipt.",
        runnable: false,
    }],
    refusals: &[
        Refusal {
            code: "workspace_not_found",
            when: "--baseline or --workspace is not a directory",
            remedy: "pass the untouched baseline and closed delivered workspace roots",
        },
        Refusal {
            code: "source_not_found",
            when: "--points is not a file",
            remedy: "pass the exact point batch used for terrain reconciliation",
        },
        Refusal {
            code: "delivery_verification_failed",
            when: "the bounded native readback finds a count, prefix, closure, or native parse mismatch",
            remedy: "read detail['task-code']; keep both workspaces immutable and repair from the baseline",
        },
        crate::RESULT_ENCODING_REFUSAL,
    ],
    reference: Some("docs/reference/pls.md"),
    availability: || Availability::Available,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let request = VerifyPlsDeliveryRequest {
        baseline_root: workspace_path(inputs.require("baseline")?)?,
        workspace_root: workspace_path(inputs.require("workspace")?)?,
        points_path: source_path(inputs.require("points")?, "points")?,
    };
    let result = verify_pls_delivery(&request).map_err(|error| {
        Failure::failed("delivery_verification_failed", error.detail)
            .remedy("read detail['task-code']; keep both workspaces immutable and repair from the baseline")
            .detail(json!({ "task-code": error.code }))
    })?;
    serde_json::to_value(result)
        .map_err(|error| Failure::internal("result_unserializable", error.to_string()))
}

pub fn render(data: &Value) -> String {
    format!(
        "PLS-CADD delivery verified {}\n  terrain {} + {} -> {} · elevation delta {}..{} m\n  alignment prefix {} · structure prefix {} · attachment closure {}\n  phase sections {} · OPGW sections {} · support chains {}/{}\n",
        data["verified"],
        data["terrain"]["baseline_point_count"],
        data["terrain"]["supplied_point_count"],
        data["terrain"]["delivered_point_count"],
        data["terrain"]["elevation_delta_min_m"],
        data["terrain"]["elevation_delta_max_m"],
        data["alignment_prefix_preserved"],
        data["structure_prefix_preserved"],
        data["attachment_closure_ready"],
        data["support_readback"]["phase_section_count"],
        data["support_readback"]["opgw_section_count"],
        data["support_readback"]["complete_support_chain_count"],
        data["support_readback"]["total_section_count"],
    )
}
