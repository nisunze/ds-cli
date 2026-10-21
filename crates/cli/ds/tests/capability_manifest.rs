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

use ds_command_kernel::capabilities::{CAPABILITIES, Executor, Route};

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
