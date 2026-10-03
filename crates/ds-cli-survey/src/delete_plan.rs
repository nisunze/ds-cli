//! IO only: preview the kernel's atomic local cascade plan from explicit rows.
use ds_cli_contract::{
    Context, Inputs,
    outcome::Failure,
    spec::{Arg, Authority, Chapter, Command, Effect, Example, Execution, Refusal, Requires},
};
use serde_json::{Value, json};
use std::{fs::OpenOptions, io::Read};
pub static COMMAND: Command = Command {
    id: "survey.entries.delete-plan",
    path: &["survey", "entries", "delete-plan"],
    contract: 1,
    chapter: Chapter::Survey,
    summary: "Preview a node and connected-edge deletion as one local plan.",
    purpose: "Inspect a bounded held-row inventory before deleting a surveyed node. Rust selects connected live edges and validates every delete intent together. This preview grants no authority, writes no entries, and cannot prove completeness beyond supplied rows. Remote replay remains individual governed mutations; it is not a server transaction. Save JSON output for review.",
    effect: Effect::ReadOnly,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[
        Arg::value(
            "project",
            "<exact-id>",
            "Explicit project of every supplied row; grants no authority.",
        )
        .required(),
        Arg::value("form", "<slug>", "Node form identity.").required(),
        Arg::value("doc-id", "<id>", "Node document identity.").required(),
        Arg::value(
            "document",
            "<json-path>",
            "Regular non-symlink JSON: prior and rows of form_slug, feature; at most 3 MiB.",
        )
        .required(),
        Arg::value(
            "idempotency-key",
            "<key>",
            "Replay identity for this exact plan, at most 480 bytes.",
        )
        .required(),
        Arg::value(
            "now",
            "<RFC3339>",
            "Device clock for the proposed tombstones.",
        )
        .required(),
    ],
    output: "One ds.survey-delete-cascade/v1 plan: intents, deleted, cascaded_edges, scope held_rows, authorized false and remote_atomic false. At most 500 targets; all validation succeeds or no plan is returned.",
    examples: &[Example {
        command: "ds survey entries delete-plan --project demo --form <form-slug> --doc-id n --document held.json --idempotency-key review-1 --now 2026-10-02T00:00:00Z --output json",
        note: "Preview supplied rows only; this does not delete anything. The exact slug comes from `ds survey forms list`.",
        runnable: false,
    }],
    refusals: &[
        Refusal {
            code: "survey_delete_document_invalid",
            when: "inventory is symlinked, nonregular, over 3 MiB or invalid JSON",
            remedy: "pass a regular bounded JSON object with prior and rows",
        },
        Refusal {
            code: "survey_delete_plan_invalid",
            when: "identity, replay key, clock or one cascade target fails validation",
            remedy: "correct the named field; use one project and at most 500 live targets",
        },
    ],
    reference: Some("docs/reference/survey.md"),
    search: &[
        "cascade",
        "connected edges",
        "remove node",
    ],
    requires: Requires::Server,
    availability: || ds_cli_contract::spec::Availability::Available,
};
fn invalid_document() -> Failure {
    Failure::invalid(
        "survey_delete_document_invalid",
        "invalid cascade inventory",
    )
    .remedy("pass a regular bounded JSON object with prior and rows")
}
pub(crate) fn load_document(path: &str) -> Result<Value, Failure> {
    let meta = std::fs::symlink_metadata(path).map_err(|_| invalid_document())?;
    if !meta.is_file() || meta.len() > 3 * 1024 * 1024 {
        return Err(invalid_document());
    }
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.custom_flags(0x0020_0000);
    }
    let file = options.open(path).map_err(|_| invalid_document())?;
    let opened = file.metadata().map_err(|_| invalid_document())?;
    if !opened.is_file() || opened.len() > 3 * 1024 * 1024 {
        return Err(invalid_document());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if meta.dev() != opened.dev() || meta.ino() != opened.ino() {
            return Err(invalid_document());
        }
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if opened.file_attributes() & 0x0000_0400 != 0 {
            return Err(invalid_document());
        }
    }
    let mut bytes = Vec::new();
    file.take(3 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| invalid_document())?;
    if bytes.len() > 3 * 1024 * 1024 {
        return Err(invalid_document());
    }
    let document: Value = serde_json::from_slice(&bytes).map_err(|_| invalid_document())?;
    if !document
        .as_object()
        .is_some_and(|o| o.keys().all(|k| matches!(k.as_str(), "prior" | "rows")))
    {
        return Err(invalid_document());
    }
    Ok(document)
}
pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let document = load_document(inputs.require("document")?)?;
    let request = json!({"operation":"mutation","mode":"delete_cascade_preview", "project_id":inputs.require("project")?,
        "form_id":inputs.require("form")?,"doc_id":inputs.require("doc-id")?,"mutation_id":inputs.require("idempotency-key")?,
        "now":inputs.require("now")?,"prior":document["prior"],"rows":document["rows"]});
    ds_command_kernel::survey::survey_evaluate(
        &serde_json::to_vec(&request).map_err(|_| invalid_document())?,
    )
    .map_err(|e| {
        Failure::invalid("survey_delete_plan_invalid", e)
            .remedy("correct the named field; use one project and at most 500 live targets")
    })
}
pub fn render(data: &Value) -> String {
    format!(
        "{} deletes · {} connected edges · local preview only\n",
        data["deleted"], data["cascaded_edges"]
    )
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn preview_has_targets_but_no_authority_or_remote_atomicity() {
        let r = ds_command_kernel::survey::survey_evaluate(&serde_json::to_vec(&json!({"operation":"mutation","mode":"delete_cascade_preview","project_id":"p","form_id":"nodes","doc_id":"n","rows":[],"mutation_id":"k","now":"2026-10-02T00:00:00Z"})).unwrap()).unwrap();
        assert_eq!(r["deleted"], 1);
        assert_eq!(r["authorized"], false);
        assert_eq!(r["remote_atomic"], false);
    }
}
