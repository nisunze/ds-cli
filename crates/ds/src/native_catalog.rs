//! Explicit full catalogue projection from registered native declarations.
use ds_cli_contract::output::{Format, Output};
use ds_cli_contract::{Context, Inputs, help};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

fn canonical(value: &Value) -> Value {
    match value {
        Value::Object(object) => Value::Object(
            object
                .iter()
                .collect::<std::collections::BTreeMap<_, _>>()
                .into_iter()
                .map(|(key, value)| (key.clone(), canonical(value)))
                .collect(),
        ),
        Value::Array(array) => Value::Array(array.iter().map(canonical).collect()),
        value => value.clone(),
    }
}

pub fn export() -> Result<Value, String> {
    let mut commands = crate::registry::all_commands();
    commands.sort_by_key(|command| command.id);
    let mut rows = commands
        .iter()
        .map(|command| {
            let mut descriptor = help::command_json_unchecked(command);
            // Help may append a navigation example. This downloadable contract
            // carries the authored Command examples without adding invocations.
            descriptor["examples"] = json!(
                command
                    .examples
                    .iter()
                    .map(|example| json!({
                        "command": example.command,
                        "note": example.note,
                        "runnable": example.runnable,
                    }))
                    .collect::<Vec<_>>()
            );
            ds_cli_mcp::cli_catalog::project_command(&descriptor)
        })
        .collect::<Result<Vec<_>, _>>()?;
    // The native owner generates this from its actual strict request type.
    // This named schema read performs no source acquisition or file writes.
    let terrain = ds_cli_data::terrain_sampling::run_describe(
        &Inputs::default(),
        &Context {
            confirmed: false,
            output: Output::resolve(Format::Json, false, true),
        },
    )
    .map_err(|error| error.to_string())?;
    let request_schema = terrain
        .get("request_schema")
        .filter(|schema| schema.is_object())
        .ok_or("native terrain request schema missing")?;
    if let Some(row) = rows
        .iter_mut()
        .find(|row| row["id"] == "data.terrain.sample")
    {
        row["request_schema"] = request_schema.clone();
    }
    let facts = serde_json::to_vec(&canonical(&json!(rows))).map_err(|error| error.to_string())?;
    Ok(json!({
        "schema":"ds.cli.tools-catalog/v1",
        "provenance": {
            "owner":"ds-cli",
            "source_revision":crate::build::SOURCE_SHA,
            "source_dirty":crate::build::dirty(),
            "generated_by":"ds capabilities --export tools --output json",
            "descriptor_sha256":format!("{:x}",Sha256::digest(&facts))
        },
        "commands":rows
    }))
}

#[cfg(test)]
mod tests {
    #[test]
    fn full_registry_export_is_exact_and_carries_native_terrain_schema() {
        let output = super::export().unwrap();
        let rows = output["commands"].as_array().unwrap();
        let commands = crate::registry::all_commands();
        assert_eq!(rows.len(), commands.len());
        for command in commands {
            let row = rows.iter().find(|row| row["id"] == command.id).unwrap();
            assert_eq!(row["purpose"], command.purpose);
            assert_eq!(
                row["cli_examples"],
                serde_json::json!(
                    command
                        .examples
                        .iter()
                        .map(|example| example.command)
                        .collect::<Vec<_>>()
                )
            );
            assert_eq!(row["execution"]["mode"], "cli");
        }
        let terrain = rows
            .iter()
            .find(|row| row["id"] == "data.terrain.sample")
            .unwrap();
        assert!(terrain["request_schema"]["properties"]["settings"].is_object());
        assert!(
            !output["provenance"]["descriptor_sha256"]
                .as_str()
                .unwrap()
                .is_empty()
        );
        assert_eq!(super::export().unwrap(), output);
    }
}
