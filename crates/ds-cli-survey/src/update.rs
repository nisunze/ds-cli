//! Argument and bounded-file IO for the native data-only Survey corrector.
use ds_cli_contract::{
    Context, Inputs,
    outcome::Failure,
    spec::{Arg, Authority, Chapter, Command, Effect, Example, Execution, Refusal, Requires},
};
use serde_json::{Value, json};
const OWN: [Refusal; 3] = [
    Refusal {
        code: "survey_update_document_invalid",
        when: "the retained document is linked, nonregular, unreadable, over 1 MiB or invalid JSON",
        remedy: "pass one regular bounded ds.survey-entry-update/v1 document with prior, changes, fields, form_revision and expected_version",
    },
    Refusal {
        code: "survey_update_plan_invalid",
        when: "identity, version, clock, changes or merged visible form fields fail native validation",
        remedy: "use the exact retained Feature and form fields, fix declared data changes and visible requirements, and retain the observed client_version (legacy missing is zero)",
    },
    Refusal {
        code: "survey_update_incomplete",
        when: "transport, version, authority or exact commit receipt fails",
        remedy: "inspect detail.failure; reread the exact entry before a new plan; never recreate it or silently rebase; an ambiguous response can mean a committed update",
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
const ARGS:[Arg;6]=[
    Arg::value("project","<exact-id>","Exact project of the retained entry.").required(),
    Arg::value("form","<slug>","Exact governed form slug.").required(),
    Arg::value("doc-id","<id>","Existing document identity; never creates a missing entry.").required(),
    Arg::value("document","<json-path>","Regular non-link JSON within 1 MiB: schema=ds.survey-entry-update/v1, prior Feature, changes object, fields, form_revision, expected_version; optional context_key.").required(),
    Arg::value("idempotency-key","<key>","Replay key for this exact reviewed correction; keep unchanged on an exact retry.").required(),
    Arg::value("now","<RFC3339>","Device audit time for this correction; capture time is preserved.").required(),
];
pub static PLAN: Command = Command {
    id: "survey.entries.update-plan",
    path: &["survey", "entries", "update-plan"],
    contract: 1,
    chapter: Chapter::Survey,
    summary: "Preview a version-fenced correction of held Survey data.",
    purpose: "Validate a retained Survey entry and its declared data-only changes with the kernel's actual form evaluator. Requires an observed expected_version (zero for legacy rows without client_version). Refuses unknown document keys, undeclared/no-op changes, retargeted rows and visible-field validation errors. Preserves geometry, connectivity and capture metadata; only named declared data fields can change. Performs no cloud read, auth or write; the retained form label is not a cloud freshness fence. Review before update.",
    effect: Effect::ReadOnly,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &ARGS,
    output: "A bounded ds.survey-entry-update-plan/v1: exact target, expected_version, changed field names, form revision/validation, preservation assertions, effects=none, authorized=false, cloud_freshness=unconfirmed. No source values or replay key.",
    examples: &[Example {
        command: "ds survey entries update-plan --project demo --form customers --doc-id n --document correction.json --idempotency-key review-1 --now 2026-10-12T00:00:00Z --output json",
        note: "Validate the held row without acquisition or effects.",
        runnable: false,
    }],
    refusals: &[OWN[0], OWN[1]],
    reference: Some("docs/reference/survey.md"),
    search: &[
        "correct",
        "classification",
        "existing entry",
        "expected version",
        "update",
    ],
    requires: Requires::Server,
    availability: || ds_cli_contract::spec::Availability::Available,
};
pub static COMMAND: Command = Command {
    id: "survey.entries.update",
    path: &["survey", "entries", "update"],
    contract: 1,
    chapter: Chapter::Survey,
    summary: "Correct declared data fields of one existing Survey entry.",
    purpose: "Commit exactly one reviewed data-only correction through the fixed native entries/mutate update operation. Native validation runs before auth; the backend authorizes the principal and checks expected_version transactionally. --yes is required. No source or geometry is fetched, no capture metadata is changed, no entry is created, and no automatic retry or conflict rebase occurs. A lost response can mean a committed write; retain the exact document, clock and replay key for an exact retry. Commit acknowledgement does not establish BigQuery mirror sync; use the existing mirror read separately.",
    effect: Effect::GlobalWrite,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[
        ARGS[0],
        ARGS[1],
        ARGS[2],
        ARGS[3],
        ARGS[4],
        ARGS[5],
        crate::LANE,
    ],
    output: "Exact target/lane and a bounded ds.survey-entry-update-receipt/v1: complete, committed receipt with new client_version, failure code/status and bigquery=unconfirmed. No request payload, source values, credential or replay key.",
    examples: &[Example {
        command: "ds survey entries update --project demo --form customers --doc-id n --document correction.json --idempotency-key review-1 --now 2026-10-12T00:00:00Z --yes --output json",
        note: "Apply the exact reviewed correction; then verify its BigQuery-backed readback.",
        runnable: false,
    }],
    refusals: REFUSALS,
    reference: Some("docs/reference/survey.md"),
    search: &[
        "correct",
        "classification",
        "existing entry",
        "expected version",
        "update",
    ],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};
fn request(inputs: &Inputs) -> Result<ds_client_core::SurveyUpdateRequest, Failure> {
    let bytes = super::validate::load_document(inputs.require("document")?).map_err(|_| {
        Failure::invalid(OWN[0].code, "invalid bounded Survey update document")
            .remedy(OWN[0].remedy)
    })?;
    let document = serde_json::from_slice(&bytes).map_err(|_| {
        Failure::invalid(OWN[0].code, "invalid JSON Survey update document").remedy(OWN[0].remedy)
    })?;
    ds_client_core::SurveyUpdateRequest::new(
        inputs.require("project")?,
        inputs.require("form")?,
        inputs.require("doc-id")?,
        inputs.require("idempotency-key")?,
        inputs.require("now")?,
        document,
    )
    .map_err(|_| {
        Failure::invalid(OWN[1].code, "invalid Survey data correction").remedy(OWN[1].remedy)
    })
}
pub fn plan(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
    Ok(request(inputs)?.preview())
}
pub fn run(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
    let request = request(inputs)?;
    let answer = ds_cli_auth::survey_update(
        inputs.require("lane")?,
        inputs.require("project")?,
        &request,
    )?;
    let lane = answer.lane();
    let mut data = answer.into_result();
    data["lane"] = json!(lane);
    if data["complete"] != true {
        return Err(Failure::unavailable(
            OWN[2].code,
            "Survey data correction was not acknowledged",
        )
        .remedy(OWN[2].remedy)
        .detail(data));
    }
    Ok(data)
}
pub fn render(data: &Value) -> String {
    format!(
        "{} / {} / {} - {} - BigQuery unconfirmed\n",
        data["project_id"].as_str().unwrap_or("?"),
        data["form_id"].as_str().unwrap_or("?"),
        data["doc_id"].as_str().unwrap_or("?"),
        if data["effects"] == "none" {
            "preview"
        } else {
            "committed"
        }
    )
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn invalid_retained_row_is_refused_before_native_auth_or_project_acquisition() {
        let path = std::env::temp_dir().join(format!(
            "ds-survey-update-invalid-{}.json",
            std::process::id()
        ));
        std::fs::write(&path, br#"{"schema":"ds.survey-entry-update/v1"}"#).unwrap();
        let raw = path.to_string_lossy();
        let args = [
            "--project",
            "p",
            "--form",
            "customers",
            "--doc-id",
            "n",
            "--document",
            &raw,
            "--idempotency-key",
            "review",
            "--now",
            "2026-10-12T00:00:00Z",
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
            "survey_update_plan_invalid"
        );
        std::fs::remove_file(path).unwrap();
        assert!(COMMAND.effect.needs_confirmation());
        assert_eq!(COMMAND.authority, Authority::HeadlessProject);
    }
}
