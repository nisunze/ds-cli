//! `ds pm record update` — change what a record says about itself: its
//! state, who owes the answer, or that no answer is expected any more.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Chapter, Command, Effect, Example, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Inputs};
use ds_client_core::project_correspondence::Action;
use serde_json::{Map, Value, json};

use super::{
    AFFECTS_ARG, BODY_ARG, BODY_FILE_ARG, PARTY_ARG, RECORD_ARG, REFERENCE_ARG, RESPONSE_DUE_ARG,
    RESPONSE_OWED_BY_ARG, SUBJECT_ARG,
};
use crate::LANE_ARG;

const STATE_ARG: Arg = Arg::value(
    "state",
    "<state>",
    "The record's state (`ds pm plan` publishes the record states).",
);
const RESPONSE_ARG: Arg = Arg::value(
    "response",
    "<waived>",
    "`waived`: no answer is expected any more; give --reason. `responded` cannot be set — it is derived from a reply.",
);
const REASON_ARG: Arg = Arg::value(
    "reason",
    "<text>",
    "Why the answer is waived (at most 300 characters).",
);

const NOTHING_TO_UPDATE: Refusal = Refusal {
    code: "nothing_to_update",
    when: "no field, state, response or party flag was given",
    remedy: "name at least one change, e.g. --response waived --reason \"agreed by phone\"",
};

pub static COMMAND: Command = Command {
    id: "pm.record.update",
    path: &["pm", "record", "update"],
    contract: 1,
    summary: "Change a record's state, who owes its answer, or waive the answer.",
    purpose: "\
Corrects what a record says about itself: its state, the parties it names, \
what it affects, its subject, body or reference, who owes the answer and by \
when — or waives the answer (--response waived --reason), which clears \
every task blocked on it in the same commit. What the record was authored \
against, its category, direction and time are fixed at creation; a waiver \
is not withdrawn; `responded` is never set by hand. The current version is \
read first and the change is refused if the record moved in between. \
Headless: writes to the selected project of the signed-in native \
credential, no window.",
    chapter: Chapter::Project,
    effect: Effect::GlobalWrite,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[
        RECORD_ARG,
        STATE_ARG,
        RESPONSE_ARG,
        REASON_ARG,
        RESPONSE_OWED_BY_ARG,
        RESPONSE_DUE_ARG,
        PARTY_ARG,
        AFFECTS_ARG,
        SUBJECT_ARG,
        BODY_ARG,
        BODY_FILE_ARG,
        REFERENCE_ARG,
        LANE_ARG,
    ],
    output: "The project and the `record` as the server now holds it, with its projections.",
    examples: &[Example {
        command: "ds pm record update --record R-0031 --response waived --reason \"answered in the site meeting of 22 Sep\" --yes",
        note: "A waiver clears the tasks blocked on R-0031 in the same commit.",
        runnable: false,
    }],
    refusals: &crate::correspondence_refusals::<31>(&[
        crate::RECORD_NOT_FOUND,
        crate::RESPONSE_NOT_SETTABLE,
        crate::INVALID_RESPONSE_OWNER,
        crate::PARTY_NOT_FOUND,
        crate::BOUND_EXCEEDED,
        crate::INVALID_DATE,
        crate::INVALID_EMAIL,
        crate::INVALID_VALUE,
        crate::BODY_UNREADABLE,
        NOTHING_TO_UPDATE,
    ]),
    reference: Some("docs/reference/pm.md"),
    search: &[
        "correspondence",
        "waive",
        "waiver",
        "close",
        "who owes",
        "ball in court",
        "response due",
        "reassign answer",
    ],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let record_id = inputs.require("record")?;
    let mut data = Map::new();
    for (flag, key) in [
        ("state", "state"),
        ("subject", "subject"),
        ("reference", "reference_number"),
    ] {
        if let Some(value) = inputs.value(flag) {
            data.insert(key.into(), json!(value.trim()));
        }
    }
    if let Some(body) = crate::body(inputs)? {
        data.insert("body".into(), json!(body));
    }
    match inputs.value("response") {
        Some("waived") => {
            let reason = inputs.value("reason").map(str::trim).unwrap_or_default();
            if reason.is_empty() {
                return Err(Failure::invalid(
                    crate::INVALID_VALUE.code,
                    "`--response waived` needs --reason",
                )
                .remedy("say why no answer is expected: --reason \"…\""));
            }
            data.insert("waived".into(), json!(true));
            data.insert("waived_reason".into(), json!(reason));
        }
        Some(other) => {
            return Err(Failure::invalid(
                crate::RESPONSE_NOT_SETTABLE.code,
                format!("`--response {other}` cannot be set; only `waived` is authored — `responded` is derived from a reply"),
            )
            .remedy(crate::RESPONSE_NOT_SETTABLE.remedy)
            .next("ds pm record reply --help"));
        }
        None => {}
    }
    if let Some(owner) = inputs.value("response-owed-by") {
        let (key, value) = crate::response_owner(owner)?;
        data.insert(key.into(), json!(value));
    }
    if let Some(due) = inputs.value("response-due") {
        data.insert(
            "response_due_date".into(),
            json!(crate::date(due, "response-due")?),
        );
    }
    let parties = inputs.repeated("party");
    if !parties.is_empty() {
        data.insert("external_party_ids".into(), json!(parties));
    }
    if let Some(affects) = inputs.value("affects") {
        for (key, flag) in crate::affects(affects)? {
            data.insert(key.into(), json!(flag));
        }
    }
    if data.is_empty() {
        return Err(
            Failure::invalid(NOTHING_TO_UPDATE.code, "no change was named")
                .remedy(NOTHING_TO_UPDATE.remedy)
                .next("ds pm record update --help"),
        );
    }
    let lane = inputs.value("lane").unwrap_or("stable");
    let current = crate::correspondence(
        lane,
        &Action::RecordRead {
            record_id: record_id.to_owned(),
        },
    )?;
    let expected_version = current.result()["item"]["version"]
        .as_i64()
        .unwrap_or(1)
        .max(1);
    let report = crate::correspondence(
        lane,
        &Action::RecordUpdate {
            id: record_id.to_owned(),
            expected_version,
            data,
        },
    )?;
    super::view(report)
}

pub fn render(data: &Value) -> String {
    let mut out = format!("updated in {}\n", data["project"].as_str().unwrap_or("?"));
    out.push_str(&super::head(&data["record"]));
    out.push_str(&super::projections(data));
    out
}
