//! Canonical shared Project Notes authoring; the CLI only adapts the owner.
use ds_cli_contract::{
    Context, Failure, Inputs,
    spec::{Arg, ArgKind, Command, Effect, Refusal},
};
use serde_json::{Value, json};

const ATTACH_ARG: Arg = Arg {
    name: "attach",
    kind: ArgKind::Repeated,
    value: "<path>",
    required: false,
    default: None,
    choices: &[],
    summary: "Attach files after creating; up to 10, 100 MiB combined, all checked first.",
};
pub(crate) const ATTACH_PARTIAL: Refusal = Refusal {
    code: "note_attachments_incomplete",
    when: "the note was created but an upload was not verified ready",
    remedy: "keep the note id; attach the missing files with `ds pm record attach`",
};

pub static CREATE: Command = Command {
    contract: 4,
    refusals: crate::note_memory::CREATE_REFUSALS,
    ..crate::note_memory::descriptor(
        "pm.note.create",
        &["pm", "note", "create"],
        "Create a sourced project note for deliberate shared memory.",
        "Consumes one closed Write JSON with operation_id, id, reason and draft(subject, body, tags, memory). Record rulings, corrections and verified outcomes deliberately; nothing is filed automatically. Memory carries provenance, scope, sourced claims and evidence. An observation may have no claims, which does not imply consensus. Model inference never becomes an accepted user decision. Optional writer model/version/strength are self-declared, distinct from the author. Creation omits expected_version; retries reuse the operation id. --attach then uploads each file onto the note as `ds pm record attach` does. No correspondence is sent.",
        Effect::GlobalWrite,
        crate::authored_args![
            crate::note_memory::INPUT,
            ATTACH_ARG,
            crate::LANE_ARG,
            crate::PROJECT_ARG
        ],
    )
};

pub fn run(i: &Inputs, c: &Context) -> Result<Value, Failure> {
    let files = crate::record::create::attachments(i)?;
    let mut data = crate::note_memory::structured_create(i, c)?;
    if files.is_empty() {
        return Ok(data);
    }
    let lane = i.value("lane").unwrap_or("stable");
    let project = i.require("project")?;
    let note = data["note"]["id"]
        .as_str()
        .filter(|id| !id.is_empty())
        .ok_or_else(|| {
            Failure::internal(
                crate::PLAN_UNREADABLE.code,
                "the created note answered no id to attach to",
            )
            .remedy(crate::PLAN_UNREADABLE.remedy)
        })?
        .to_owned();
    let mut uploads = Vec::new();
    for (index, file) in files.iter().enumerate() {
        match crate::record::attach::upload(lane, project, &note, file) {
            Ok(receipt) => uploads.push(receipt),
            Err(failure) => {
                return Err(Failure::failed(
                    ATTACH_PARTIAL.code,
                    "Note created; not all attachments were verified ready",
                )
                .remedy(ATTACH_PARTIAL.remedy)
                .detail(
                    json!({"project": project, "noteId": note, "uploads": uploads,
                    "failed_attachment_index": index, "requested_attachment_count": files.len(),
                    "failure_code": failure.code()}),
                ));
            }
        }
    }
    data["uploads"] = Value::Array(uploads);
    Ok(data)
}

pub fn render(data: &Value) -> String {
    let mut out = format!(
        "created project note {} (version {}) in {}\n  {}\n",
        data["note"]["id"].as_str().unwrap_or("?"),
        data["note"]["version"].as_i64().unwrap_or(0),
        data["project_id"].as_str().unwrap_or("?"),
        data["note"]["document"]["data"]["subject"]
            .as_str()
            .unwrap_or("?"),
    );
    for upload in data["uploads"].as_array().into_iter().flatten() {
        out.push_str(&format!(
            "  uploaded {} · {} bytes · {}\n",
            upload["attachment"]["original_name"]
                .as_str()
                .unwrap_or("?"),
            upload["bytes"].as_u64().unwrap_or(0),
            upload["attachment"]["state"].as_str().unwrap_or("?"),
        ));
    }
    out
}

#[cfg(test)]
#[path = "note_tests.rs"]
mod tests;
