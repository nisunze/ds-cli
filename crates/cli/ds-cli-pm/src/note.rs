//! Canonical shared Project Notes authoring; the CLI only adapts the owner.
use ds_cli_contract::{
    Context, Failure, Inputs,
    spec::{Command, Effect},
};
use serde_json::Value;

pub static CREATE: Command = Command {
    contract: 4,
    ..crate::note_memory::descriptor(
        "pm.note.create",
        &["pm", "note", "create"],
        "Create a sourced project note for deliberate shared memory.",
        "Consumes one closed Write JSON with operation_id, id, reason and draft(subject, body, tags, memory). Record rulings, corrections and verified outcomes deliberately; nothing is filed automatically. Memory carries provenance, scope, sourced claims and evidence. An observation may have no claims, which does not imply consensus. Model inference never becomes an accepted user decision. Optional writer model/version/strength are self-declared, distinct from the author. Creation omits expected_version; retries reuse the operation id. No correspondence is sent.",
        Effect::GlobalWrite,
        crate::authored_args![
            crate::note_memory::INPUT,
            crate::LANE_ARG,
            crate::PROJECT_ARG
        ],
    )
};

pub fn run(i: &Inputs, c: &Context) -> Result<Value, Failure> {
    crate::note_memory::structured_create(i, c)
}

pub fn render(data: &Value) -> String {
    format!(
        "created project note {} (version {}) in {}\n  {}\n",
        data["note"]["id"].as_str().unwrap_or("?"),
        data["note"]["version"].as_i64().unwrap_or(0),
        data["project_id"].as_str().unwrap_or("?"),
        data["note"]["document"]["data"]["subject"]
            .as_str()
            .unwrap_or("?"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn creation_has_one_canonical_required_input() {
        assert_eq!(CREATE.effect, Effect::GlobalWrite);
        assert!(CREATE.args.iter().any(|a| a.name == "input" && a.required));
        assert!(!CREATE.args.iter().any(|a| matches!(
            a.name,
            "title" | "body" | "body-file" | "id" | "transformer"
        )));
        assert!(ds_cli_contract::args::parse(&CREATE, &["--project".into(), "p".into()]).is_err());
    }
}
