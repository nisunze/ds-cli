//! A downloadable CLI-contract projection. No execution or availability probe.
use serde_json::{Value, json};

/// Reuse the MCP input projection so flags, choices and confirmation semantics
/// have one schema implementation. The CLI examples remain exactly authored.
pub fn project_command(descriptor: &Value) -> Result<Value, String> {
    let id = descriptor
        .get("id")
        .and_then(Value::as_str)
        .ok_or("command ID missing")?;
    let tool = crate::tools::tool_from_descriptor(descriptor)
        .ok_or_else(|| format!("{id}: declared command cannot be projected faithfully"))?;
    let purpose = descriptor
        .get("purpose")
        .and_then(Value::as_str)
        .ok_or_else(|| format!("{id}: declared purpose missing"))?;
    let examples = descriptor
        .get("examples")
        .and_then(Value::as_array)
        .ok_or_else(|| format!("{id}: declared examples missing"))?;
    let cli_examples = examples
        .iter()
        .map(|example| {
            example
                .get("command")
                .and_then(Value::as_str)
                .map(str::to_string)
                .ok_or_else(|| format!("{id}: example command missing"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(json!({
        "id": id,
        "chapter": descriptor["chapter"],
        "summary": descriptor["summary"],
        "purpose": purpose,
        "status": "declared",
        "availability": "deferred",
        "cli_examples": cli_examples,
        "arguments_schema": tool.input_schema,
        "cli_path": descriptor["path"],
        "contract": descriptor["contract"],
        "effect": descriptor["effect"],
        "authority": descriptor["authority"],
        "requires": descriptor["requires"],
        "output_contract": descriptor["output"],
        "refusals": descriptor["refusals"],
        "execution": {
            "mode": "cli",
            "reason": "Use the declared native CLI invocation; this catalogue supplies no browser execution binding.",
            "native_mode": descriptor["execution"]
        }
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn descriptor() -> Value {
        json!({"id":"data.example","path":["data","example"],"chapter":"data","summary":"Declared example",
            "purpose":"Read the caller's declared source.\nKeep its exact ‘native’ wording, including spaces and \"quotes\".\n",
            "contract":1,"effect":"local_file_write","authority":"none","execution":"sync","requires":"server",
            "availability":"unchecked","confirmation_required":false,"output":"Exact native receipt","refusals":[],
            "inputs":[{"name":"source","kind":"value","required":true,"summary":"Source CSV","value":"<file>"},
                {"name":"method","kind":"value","required":false,"summary":"Interpolation","value":"<method>","choices":["nearest","bilinear"],"default":"bilinear"},
                {"name":"dry-run","kind":"switch","required":false,"summary":"Validate"}],
            "examples":[{"command":"ds data example --source measured.csv --dry-run --output json","note":"Actual authored example","runnable":false}]})
    }
    #[test]
    fn catalogue_preserves_declared_purpose_examples_and_mcp_argument_schema() {
        let input = descriptor();
        let output = project_command(&input).unwrap();
        assert_eq!(
            output["arguments_schema"],
            crate::tools::tool_from_descriptor(&input)
                .unwrap()
                .input_schema
        );
        assert_eq!(output["cli_examples"][0], input["examples"][0]["command"]);
        assert_eq!(output["purpose"], input["purpose"]);
        assert_eq!(
            output["arguments_schema"]["properties"]["method"]["enum"],
            json!(["nearest", "bilinear"])
        );
        assert_eq!(output["arguments_schema"]["required"], json!(["source"]));
        assert_eq!(output["status"], "declared");
        assert_eq!(output["availability"], "deferred");
        assert_eq!(output["execution"]["mode"], "cli");
        assert!(output.get("request_schema").is_none());
        assert!(output.get("result_schema").is_none());
    }
    #[test]
    fn invalid_or_global_flag_colliding_contract_refuses_instead_of_inventing_schema() {
        let mut input = descriptor();
        input["inputs"][0]["name"] = json!("output");
        assert!(project_command(&input).is_err());
        input = descriptor();
        input["execution"] = json!("browser");
        assert!(project_command(&input).is_err());
        input = descriptor();
        input.as_object_mut().unwrap().remove("purpose");
        assert_eq!(
            project_command(&input).unwrap_err(),
            "data.example: declared purpose missing"
        );
    }
}
