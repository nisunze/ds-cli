//! Bounded local IO for the shared runtime field-rule validator.
use ds_cli_contract::{
    Context, Inputs,
    outcome::Failure,
    spec::{Arg, Authority, Chapter, Command, Effect, Example, Execution, Refusal, Requires},
};
use serde_json::Value;
use std::{fs::OpenOptions, io::Read};

pub static COMMAND: Command = Command {
    id: "survey.form.validate",
    path: &["survey", "form", "validate"],
    contract: 1,
    chapter: Chapter::Survey,
    summary: "Preview a retained form's visible and required field rules.",
    purpose: "Check proposed values with the Rust field evaluator Survey submission uses, from one ds.survey-form-validation/v1 document (at most 1 MiB, 512 fields); reports visible and hidden fields and errors. No entry, fetch, sign-in or freshness check; hidden required fields never block.",
    effect: Effect::ReadOnly,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[Arg::value("document", "<json-path>", "Regular UTF-8 JSON file within 1 MiB: ds.survey-form-validation/v1 with form_revision, fields and values.").required()],
    output: "ds.survey-form-validation-result/v1: valid, visible_keys, hidden_keys, required_visible_keys, errors, form_revision, input_sha256, effects=none. Invalid values give valid=false; malformed requests refuse. Values are never echoed.",
    examples: &[Example { command: "ds survey form validate --document form-values.json --output json", note: "Preview a retained form revision and values; no acquisition or write.", runnable: false }],
    refusals: &[
        Refusal { code: "survey_form_validation_document_invalid", when: "the input is missing, linked, nonregular, unreadable or over 1 MiB", remedy: "use one regular non-link UTF-8 validation document within 1 MiB" },
        Refusal { code: "survey_form_validation_invalid", when: "the request, revision label or field schema violates the validator", remedy: "use ds.survey-form-validation/v1 with a retained form_revision, at most 512 fields and values; fix the named field" },
    ],
    reference: Some("docs/reference/survey.md"),
    search: &["conditional", "hidden", "validation"],
    requires: Requires::Server,
    availability: || ds_cli_contract::spec::Availability::Available,
};

fn invalid_document() -> Failure {
    Failure::invalid(
        "survey_form_validation_document_invalid",
        "invalid bounded form-validation document",
    )
    .remedy("use one regular non-link UTF-8 validation document within 1 MiB")
}
pub(crate) fn load_document(path: &str) -> Result<Vec<u8>, Failure> {
    let metadata = std::fs::symlink_metadata(path).map_err(|_| invalid_document())?;
    if !metadata.is_file()
        || metadata.len() > ds_command_kernel::survey::MAX_SURVEY_INPUT_BYTES as u64
    {
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
    if !opened.is_file() || opened.len() > ds_command_kernel::survey::MAX_SURVEY_INPUT_BYTES as u64
    {
        return Err(invalid_document());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if metadata.dev() != opened.dev() || metadata.ino() != opened.ino() {
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
    file.take(ds_command_kernel::survey::MAX_SURVEY_INPUT_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| invalid_document())?;
    if bytes.len() > ds_command_kernel::survey::MAX_SURVEY_INPUT_BYTES {
        return Err(invalid_document());
    }
    Ok(bytes)
}
pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let bytes = load_document(inputs.require("document")?)?;
    evaluate_document(&bytes)
}
fn evaluate_document(bytes: &[u8]) -> Result<Value, Failure> {
    ds_command_kernel::survey::preview::validate(bytes).map_err(|message| {
        Failure::invalid("survey_form_validation_invalid", message)
            .remedy("use ds.survey-form-validation/v1 with a retained form_revision, at most 512 fields and values; fix the named field")
    })
}
pub fn render(data: &Value) -> String {
    format!(
        "{} - {} visible fields - revision {}\n",
        if data["valid"] == true {
            "valid"
        } else {
            "invalid"
        },
        data["visible_keys"].as_array().map_or(0, Vec::len),
        data["form_revision"].as_str().unwrap_or("")
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn document(values: Value) -> Vec<u8> {
        serde_json::to_vec(&json!({"schema":"ds.survey-form-validation/v1","form_revision":"retained:17","fields":[
            {"key":"category","type":"radio","label":"Category","required":true,"options":["Residential","Productive"]},
            {"key":"type","type":"radio","label":"Nature","required":true,"options":["School","Other"],"visibility":{"conditions":[{"field":"category","op":"equals","value":"Productive"}],"mode":"all","showWhenTrue":true}},
            {"key":"other_detail","type":"text","label":"Other detail","required":true,"visibility":{"conditions":[{"field":"category","op":"equals","value":"Productive"},{"field":"type","op":"equals","value":"Other"}],"mode":"all","showWhenTrue":true}}
        ],"values":values})).unwrap()
    }
    #[test]
    fn actual_runtime_validator_checks_conditional_requirements_without_effects() {
        for (values, valid, error_key) in [
            (json!({"category":"Residential"}), true, None),
            (json!({"category":"Residential","type":"Other"}), true, None),
            (json!({"category":"Productive"}), false, Some("type")),
            (
                json!({"category":"Productive","type":"Other"}),
                false,
                Some("other_detail"),
            ),
            (json!({"category":"Productive","type":"School"}), true, None),
            (
                json!({"category":"Productive","type":"Other","other_detail":"Farm"}),
                true,
                None,
            ),
        ] {
            let result = evaluate_document(&document(values)).unwrap();
            assert_eq!(result["valid"], valid);
            if let Some(key) = error_key {
                assert!(result["errors"][key].is_string());
            }
            assert_eq!(result["effects"], "none");
            assert_eq!(result["form_revision"], "retained:17");
            assert!(result.get("data").is_none());
            assert!(result.get("values").is_none());
        }
    }
    #[test]
    fn closed_request_refuses_unrelated_operation_and_preserves_digest() {
        let bytes = document(json!({"category":"Residential"}));
        let first = evaluate_document(&bytes).unwrap();
        let second = evaluate_document(&bytes).unwrap();
        assert_eq!(first["input_sha256"], second["input_sha256"]);
        let mut value: Value = serde_json::from_slice(&bytes).unwrap();
        value["url"] = json!("https://invalid.example/");
        let error = evaluate_document(&serde_json::to_vec(&value).unwrap()).unwrap_err();
        assert_eq!(error.code(), "survey_form_validation_invalid");
    }
}
