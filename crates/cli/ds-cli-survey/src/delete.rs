//! IO and acknowledgement display for the native kernel cascade executor.
use ds_cli_contract::{
    Context, Inputs,
    outcome::Failure,
    spec::{Authority, Chapter, Command, Effect, Example, Execution, Refusal, Requires},
};
use serde_json::{Value, json};
const OWN: [Refusal; 3] = [
    Refusal {
        code: "survey_delete_document_invalid",
        when: "held inventory is not a regular bounded closed JSON document",
        remedy: "pass the same regular non-symlink prior/rows file used for delete-plan, at most 3 MiB",
    },
    Refusal {
        code: "survey_delete_plan_invalid",
        when: "one identity, clock, replay key or target fails the kernel's full-plan validation",
        remedy: "review delete-plan and correct its input before replaying",
    },
    Refusal {
        code: "survey_delete_incomplete",
        when: "a transport, version, authority or receipt failure stops remote replay",
        remedy: "inspect detail.failure and acknowledged receipts; recheck authority or version; never change the file, key or clock for an exact retry",
    },
];
const REFUSALS: &[Refusal] = &{
    let mut all = [OWN[0]; crate::COMMON_REFUSALS.len() + OWN.len()];
    let mut i = 0;
    while i < crate::COMMON_REFUSALS.len() {
        all[i] = crate::COMMON_REFUSALS[i];
        i += 1;
    }
    let mut j = 0;
    while j < OWN.len() {
        all[i + j] = OWN[j];
        j += 1;
    }
    all
};
pub static COMMAND: Command = Command {
    id: "survey.entries.delete",
    path: &["survey", "entries", "delete"],
    contract: 1,
    chapter: Chapter::Survey,
    summary: "Delete a reviewed node and its held connected edges headlessly.",
    purpose: "Remove a surveyed node and the live connected edges in an explicit held-row inventory. Review delete-plan first. All targets validate together before auth or transport; --yes then replays the exact plan through the governed native user client, with backend authorization per mutation. The saved project is never read. This cannot establish complete server connectivity: only the supplied inventory is covered. Remote deletes are sequential, not atomic, and stop at the first unacknowledged target. No automatic retry or conflict rebase occurs. Preserve the exact file, key and clock for a replay; a lost response can mean a committed write. Individual backend replay receipts are not atomically recorded with the mutation. BigQuery readback remains unconfirmed.",
    effect: Effect::GlobalWrite,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[
        crate::PROJECT,
        super::delete_plan::COMMAND.args[1],
        super::delete_plan::COMMAND.args[2],
        super::delete_plan::COMMAND.args[3],
        super::delete_plan::COMMAND.args[4],
        super::delete_plan::COMMAND.args[5],
        crate::LANE,
    ],
    output: "Bounded acknowledgement only: lane, exact project, targets, receipts, complete, held_rows scope and remote_atomic false. Stops on failure; error detail retains confirmed receipts, failed index and status. Acknowledged absent/deleted no-ops remain distinct from a new delete. BigQuery unconfirmed. No request payload, credential or replay key is returned.",
    examples: &[Example {
        command: "ds survey entries delete --project demo --form <form-slug> --doc-id n --document held.json --idempotency-key reviewed-1 --now 2026-10-02T00:00:00Z --yes --output json",
        note: "Replay exactly the reviewed held-row plan; individual remote deletes are not a transaction. The exact slug comes from `ds survey forms list`.",
        runnable: false,
    }],
    refusals: REFUSALS,
    reference: Some("docs/reference/survey.md"),
    search: &[
        "delete entry",
        "remove node",
        "cascade",
        "connected edges",
        "delete pole",
    ],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};
pub fn run(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
    let project = inputs.require("project")?;
    let request = ds_client_core::SurveyDeleteRequest::new(
        project,
        inputs.require("form")?,
        inputs.require("doc-id")?,
        inputs.require("idempotency-key")?,
        inputs.require("now")?,
        super::delete_plan::load_document(inputs.require("document")?)?,
    )
    .map_err(|_| {
        Failure::invalid(OWN[1].code, "invalid Survey delete plan").remedy(OWN[1].remedy)
    })?;
    let answer = ds_cli_auth::survey_delete(inputs.require("lane")?, project, &request)?;
    let lane = answer.lane();
    let mut data = answer.into_result();
    data["lane"] = json!(lane);
    if data["complete"] != true {
        return Err(Failure::unavailable(
            OWN[2].code,
            "Survey delete replay stopped; confirmed progress is in detail",
        )
        .remedy(OWN[2].remedy)
        .detail(data));
    }
    Ok(data)
}
pub fn render(data: &Value) -> String {
    format!(
        "{} · {} acknowledged deletes · BigQuery unconfirmed · individual remote mutations\n",
        data["project_id"].as_str().unwrap_or("?"),
        data["acknowledged"]
    )
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn deletion_requires_explicit_confirmation_and_native_project_authority() {
        assert!(COMMAND.effect.needs_confirmation());
        assert_eq!(COMMAND.authority, Authority::HeadlessProject);
        assert!(COMMAND.summary.len() < 70);
    }
    #[test]
    fn handler_rejects_an_invalid_full_plan_before_loading_native_authority() {
        let path = std::env::temp_dir().join(format!(
            "ds-survey-delete-invalid-{}.json",
            std::process::id()
        ));
        std::fs::write(&path, br#"{"rows":[]}"#).unwrap();
        let raw = path.to_string_lossy();
        let args = [
            "--project",
            "p",
            "--form",
            "poles",
            "--doc-id",
            "n",
            "--document",
            &raw,
            "--idempotency-key",
            "review",
            "--now",
            "invalid-clock",
        ];
        let inputs = ds_cli_contract::parse(
            &COMMAND,
            &args.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
        )
        .unwrap();
        let context = Context {
            confirmed: true,
            output: ds_cli_contract::Output::resolve(ds_cli_contract::Format::Json, false, true),
        };
        assert_eq!(
            run(&inputs, &context).unwrap_err().code(),
            "survey_delete_plan_invalid"
        );
        std::fs::remove_file(path).unwrap();
    }
}
