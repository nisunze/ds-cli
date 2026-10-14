//! Read-only addon inventory and Kernel-owned release metadata admission.
use ds_cli_contract::spec::{
    Arg, Authority, Availability, Chapter, Command, Effect, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Failure, Inputs};
use serde_json::Value;
use std::{fs::File, io::Read};

pub static INVENTORY: Command = Command {
    id: "core.modules",
    path: &["core", "modules"],
    contract: 1,
    summary: "Inspect installed Solar, Reporter and Network engine addons.",
    purpose: "Inspect the three fixed native addon executables, their captured build identities and declared capabilities. Starts at most one build-info task per available addon on intentional inspection; help and command discovery start none. Missing or incompatible engines remain named rows. This does not install, update, acquire project data, or establish full application workflow readiness.",
    chapter: Chapter::Design,
    effect: Effect::Discovery,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[Arg::value(
        "component",
        "<solar|reporter|network>",
        "Inspect only this consumed addon; omit for all three.",
    )
    .choices(&["solar", "reporter", "network"])],
    output: "ds.module-inventory/v1 with Solar, Reporter and Network rows, or exactly the selected component: available, missing or refused, captured path, identity and capabilities or a named remedy. Development provenance remains explicit.",
    examples: &[],
    refusals: &[],
    reference: Some("docs/reference/core.md"),
    search: &["addons", "extensions"],
    requires: Requires::Server,
    availability: || Availability::Available,
};

pub static RELEASE: Command = Command {
    id: "core.module-release",
    path: &["core", "module-release"],
    contract: 1,
    summary: "Validate one module's declared release metadata.",
    purpose: "Read bounded local JSON and ask the Kernel to validate the selected module and lane's release metadata. This is read-only schema and compatibility inspection; it grants no installation trust, download, activation, computation or publication authority.",
    chapter: Chapter::Design,
    effect: Effect::ReadOnly,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[
        Arg::value(
            "component",
            "<solar|reporter|network>",
            "Exact addon identity.",
        )
        .choices(&["solar", "reporter", "network"])
        .required(),
        Arg::value("channel", "<stable|canary>", "Exact release lane.")
            .choices(&["stable", "canary"])
            .required(),
        Arg::value(
            "file",
            "<json-file>",
            "Local module manifest or public release envelope; at most 2 MiB.",
        )
        .required(),
    ],
    output: "The Kernel-admitted module manifest, or null when no release was declared. Never a verified install receipt.",
    examples: &[],
    refusals: &[Refusal {
        code: "module_release_invalid",
        when: "the local file, manifest, selected module or lane is invalid, excessive or incompatible",
        remedy: "retain the metadata; select its exact module and lane or obtain a corrected declared release",
    }],
    reference: Some("docs/reference/core.md"),
    search: &["addons", "release"],
    requires: Requires::Server,
    availability: || Availability::Available,
};

pub fn inventory(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
    let component = inputs
        .value("component")
        .map(|value| serde_json::from_value(serde_json::json!(value)))
        .transpose()
        .map_err(|_| {
            Failure::invalid(
                "module_component_invalid",
                "Select a declared module component",
            )
        })?;
    inventory_for_release_lane(component, ds_cli_auth::reliability::release_lane())
}

fn inventory_for_release_lane(
    component: Option<ds_command_kernel::module_release::ModuleComponent>,
    lane: &str,
) -> Result<Value, Failure> {
    if lane == "local" {
        return Ok(ds_cli_exec::addons::inventory_for(component));
    }
    let mut inventory = ds_cli_exec::addons::inventory_for_lane(component, lane)?;
    if let Some(rows) = inventory["modules"].as_array_mut() {
        for row in rows {
            let selected: ds_command_kernel::module_release::ModuleComponent =
                serde_json::from_value(row["component"].clone()).map_err(|_| {
                    Failure::failed(
                        "module_inventory_invalid",
                        "The native inventory returned an invalid component",
                    )
                })?;
            // Corrupt/unavailable managed owners are already named inventory rows.
            let Ok(store) = ds_update_runtime::modules::ModuleStore::managed(selected, lane) else {
                continue;
            };
            let Ok(Some(receipt)) = store.selection() else {
                continue;
            };
            let asset = ds_command_kernel::module_install::selected_asset(&receipt.active)
                .map_err(|e| Failure::failed("module_inventory_invalid", e))?;
            let executable = ds_command_kernel::module_install::executable_name(
                selected,
                if cfg!(target_os = "windows") {
                    "windows"
                } else {
                    "linux"
                },
            )
            .map_err(|e| Failure::failed("module_inventory_invalid", e))?;
            let path = ds_cli_exec::addons::managed_module_base()?
                .join(selected.as_str())
                .join(lane)
                .join("versions")
                .join(&asset.sha256)
                .join(executable);
            // Explicit development overrides cannot borrow an installed receipt.
            if std::fs::canonicalize(&path)
                .ok()
                .as_deref()
                .and_then(std::path::Path::to_str)
                == row["path"].as_str()
                && row["path"].is_string()
            {
                row["installation"] = serde_json::json!({
                    "version": receipt.active.release.version,
                    "released_at": receipt.active.release.released_at,
                    "generation": receipt.generation,
                    "sha256": asset.sha256,
                    "previousRecorded": receipt.previous.is_some()
                });
            }
        }
    }
    Ok(inventory)
}

pub fn release(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
    const LIMIT: u64 = 2 * 1024 * 1024;
    let invalid = |message: String| {
        Failure::invalid("module_release_invalid", message)
        .remedy("retain the metadata; select its exact module and lane or obtain a corrected declared release")
    };
    let path = inputs.require("file")?;
    let metadata = std::fs::symlink_metadata(path).map_err(|error| invalid(error.to_string()))?;
    if !metadata.is_file() || metadata.len() > LIMIT {
        return Err(invalid(
            "release metadata must be a bounded regular file".into(),
        ));
    }
    let mut bytes = Vec::new();
    File::open(path)
        .map_err(|error| invalid(error.to_string()))?
        .take(LIMIT + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| invalid(error.to_string()))?;
    if bytes.len() as u64 > LIMIT {
        return Err(invalid("release metadata grew beyond its bound".into()));
    }
    let value: Value =
        serde_json::from_slice(&bytes).map_err(|error| invalid(error.to_string()))?;
    let value = value.get("release").unwrap_or(&value);
    if value.is_null() {
        return Ok(Value::Null);
    }
    let admitted = ds_command_kernel::module_release::validate_module_release(
        inputs.require("component")?,
        inputs.require("channel")?,
        value,
    )
    .map_err(|error| invalid(format!("{}: {}", error.code, error.message)))?;
    serde_json::to_value(admitted).map_err(|error| invalid(error.to_string()))
}

#[cfg(test)]
mod inventory_tests {
    use super::*;
    #[test]
    fn unsupported_release_lane_refuses_before_any_addon_probe() {
        let error = inventory_for_release_lane(
            Some(ds_command_kernel::module_release::ModuleComponent::Network),
            "server",
        )
        .unwrap_err();
        assert_eq!(error.code(), "module_lane_invalid");
    }
}
