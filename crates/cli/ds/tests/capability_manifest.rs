//! Does every Core command have a decision for every platform?
//!
//! The Kernel compiles `ds-command-kernel/routing/capabilities.json` into
//! `ds_command_kernel::capabilities::CAPABILITIES`, and its build refuses a
//! row that does not decide every declared platform. The rows it supports on
//! `cli` must be exactly the commands this binary serves, read the way an
//! agent reads them (`ds capabilities --export tools`). A Core command
//! without such a row, and a row naming a command Core does not have, each
//! fail here by name. A capability Core does not carry is a row whose `cli`
//! is pending or discarded.

use std::collections::BTreeSet;

use ds_command_kernel::capabilities::{CAPABILITIES, Executor, Route, Run, command};
use serde_json::{Map, Value, json};

mod common;

fn served() -> BTreeSet<String> {
    let (export, code) = common::json(&["capabilities", "--export", "tools", "--output", "json"]);
    assert_eq!(code, 0, "the tools export failed: {export}");
    export["data"]["commands"]
        .as_array()
        .expect("the export lists its commands")
        .iter()
        .map(|command| command["id"].as_str().expect("command id").to_owned())
        .collect()
}

fn supported_on_cli() -> BTreeSet<String> {
    CAPABILITIES
        .iter()
        .filter(|capability| {
            matches!(
                capability.route_on("cli"),
                Some(Route::Supported {
                    executor: Executor::Core,
                    ..
                })
            )
        })
        .map(|capability| capability.id.to_owned())
        .collect()
}

#[test]
fn the_manifest_runs_exactly_cores_commands_on_cli() {
    let served = served();
    let declared = supported_on_cli();
    let missing: Vec<&String> = served.difference(&declared).collect();
    let unknown: Vec<&String> = declared.difference(&served).collect();
    assert!(
        missing.is_empty(),
        "Core serves commands that ds-command-kernel/routing/capabilities.json does not run on \
         cli; add one row each with a value for every platform: {missing:?}"
    );
    assert!(
        unknown.is_empty(),
        "ds-command-kernel/routing/capabilities.json runs these on cli, but Core has no such \
         command; remove the row, or mark cli pending or discarded: {unknown:?}"
    );
}

fn descriptor(id: &str) -> Value {
    let (descriptor, code) = common::json(&["capabilities", id, "--output", "json"]);
    assert_eq!(code, 0, "Core did not describe `{id}`: {descriptor}");
    descriptor["data"]["command"].clone()
}

/// Check names against the actual descriptor, then let the shared admission
/// owner check required inputs, defaults and native context. An omitted caller
/// document checks metadata alone: a direct-copy run can receive its arguments
/// from a form, whereas a wrapped document has its fixed input name in `run`.
fn check_run(run: Option<Run>, descriptor: &Value, caller: Option<&Value>) -> Result<(), String> {
    let id = descriptor["id"].as_str().ok_or("descriptor has no id")?;
    if let Some(run) = run {
        let inputs = descriptor["inputs"]
            .as_array()
            .ok_or_else(|| format!("`{id}` has no declared inputs"))?;
        for (member, name) in [("input", run.input), ("output", run.output)] {
            if let Some(name) = name {
                let input = inputs
                    .iter()
                    .find(|input| input["name"] == name)
                    .ok_or_else(|| format!("`{id}` run.{member} names undeclared `{name}`"))?;
                if input["kind"] != "value" {
                    return Err(format!("`{id}` run.{member} `{name}` is not a value input"));
                }
            }
        }
        for name in run.switches {
            if !inputs
                .iter()
                .any(|input| input["name"] == *name && input["kind"] == "switch")
            {
                return Err(format!(
                    "`{id}` run switch `{name}` is not a declared switch"
                ));
            }
        }
    }
    let Some(caller) = caller else {
        return Ok(());
    };
    let mut args = match run.and_then(|run| run.input) {
        Some(name) => Map::from_iter([(name.to_owned(), caller.clone())]),
        None => caller
            .as_object()
            .cloned()
            .ok_or("direct-copy arguments must be an object")?,
    };
    for switch in run.map_or(&[][..], |run| run.switches) {
        args.insert((*switch).to_owned(), Value::Bool(true));
    }
    if let Some(output) = run.and_then(|run| run.output) {
        args.insert(output.to_owned(), Value::Bool(true));
    }
    match command::admit(id, descriptor, &Value::Object(args), "parity-project") {
        Some(refusal) => Err(format!("`{id}`: {}: {}", refusal.code, refusal.message)),
        None => Ok(()),
    }
}

#[test]
fn manifest_run_metadata_matches_live_command_descriptors() {
    let document = json!({});
    let mut checked = 0;
    for capability in CAPABILITIES {
        let Some(run) = capability.run else {
            continue;
        };
        // A named input fixes the complete request shape. Direct-copy callers
        // retain their declared arguments; specific empty requests are proved below.
        let caller = run.input.map(|_| &document);
        check_run(Some(run), &descriptor(capability.id), caller)
            .unwrap_or_else(|error| panic!("capability run/descriptor mismatch: {error}"));
        checked += 1;
    }
    assert!(checked > 0, "the manifest declares no page run contracts");
}

#[test]
fn manifest_run_settings_catalog_request_needs_no_caller_arguments() {
    let capability =
        ds_command_kernel::capabilities::capability("design.process.settings").unwrap();
    check_run(capability.run, &descriptor(capability.id), Some(&json!({}))).unwrap();
}

#[test]
fn manifest_run_required_preset_drift_is_rejected_before_execution() {
    let capability =
        ds_command_kernel::capabilities::capability("design.process.settings").unwrap();
    let mut descriptor = descriptor(capability.id);
    let preset = descriptor["inputs"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|input| input["name"] == "preset")
        .expect("the command declares its preset");
    preset["required"] = Value::Bool(true);
    preset.as_object_mut().unwrap().remove("default");
    let refusal = check_run(capability.run, &descriptor, Some(&json!({}))).unwrap_err();
    assert!(refusal.contains("inputs_invalid"), "{refusal}");
    assert!(refusal.contains("requires `preset`"), "{refusal}");
}

#[test]
fn manifest_run_omission_preserves_declared_copy_arguments() {
    let descriptor = descriptor("design.process.settings");
    check_run(None, &descriptor, Some(&json!({"catalog": true}))).unwrap();
}
