//! `ds pls desktop dialogs` — the dialog catalogue the drivers decide by.
//!
//! The rule it serves: an unknown dialog stops a run. It is recorded in the
//! catalogue with its decision, never clicked through blind. This command
//! lists the decisions, before a run or after an `unknown_dialog` or
//! `dialog_stop` refusal, from the catalogue embedded in this `ds` — the one
//! its drivers will use. It only reads, so it answers on any host.

use ds_cli_contract::spec::{
    Arg, Authority, Availability, Chapter, Command, Effect, Example, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Failure, Inputs};
use serde_json::{Value, json};

use super::catalog::{self, ACTIONS};

pub const DIALOG_NOT_FOUND: Refusal = Refusal {
    code: "dialog_not_found",
    when: "--name is not a catalogued dialog",
    remedy: "list the catalogue with `ds pls desktop dialogs` and use an entry's name",
};
pub const CATALOG_UNREADABLE: Refusal = Refusal {
    code: "catalog_unreadable",
    when: "the embedded catalogue cannot be read",
    remedy: "this build of ds is broken; reinstall it and report the build",
};

pub const RULE: &str = "An unknown dialog stops the run. Record it in the catalogue with its decision; never click through it blind.";

pub static COMMAND: Command = Command {
    id: "pls.desktop.dialogs",
    path: &["pls", "desktop", "dialogs"],
    contract: 1,
    summary: "List the PLS-CADD dialogs the desktop drivers know, and each decision.",
    purpose: "Shows the dialog catalogue the desktop drivers act on: every PLS-CADD 16.81 modal they have met, when it fires and what they do — wait, press a named button, drive it, or stop. A dialog not in the catalogue stops a run with unknown_dialog. Read an entry with --name after a dialog_stop or unknown_dialog refusal, or before a run to see what will be answered automatically. Reads the catalogue embedded in this ds, on any host.",
    chapter: Chapter::PlsCadd,
    effect: Effect::Discovery,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[
        Arg::value(
            "name",
            "<dialog>",
            "One entry in full: its title and text patterns and its note.",
        ),
        Arg::value(
            "action",
            "<decision>",
            "Only the entries with this decision.",
        )
        .choices(ACTIONS),
    ],
    output: "The catalogue version, the rule, and each entry's name, decision, button control id and when it fires; with --name, that one entry with its title and text patterns and its note.",
    examples: &[
        Example {
            command: "ds pls desktop dialogs",
            note: "Every decision the drivers will take.",
            runnable: true,
        },
        Example {
            command: "ds pls desktop dialogs --name save_changes",
            note: "Exit answers 'Save changes' with No (id 7).",
            runnable: true,
        },
        Example {
            command: "ds pls desktop dialogs --action stop",
            note: "The dialogs that stop an unattended run.",
            runnable: true,
        },
    ],
    refusals: &[DIALOG_NOT_FOUND, CATALOG_UNREADABLE],
    reference: Some("docs/reference/pls.md"),
    search: &["dialog catalogue", "unknown dialog", "pls-cadd prompts"],
    requires: Requires::Server,
    availability: || Availability::Available,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let catalog = catalog::embedded().map_err(|detail| {
        Failure::internal(
            CATALOG_UNREADABLE.code,
            "the embedded catalogue cannot be read",
        )
        .remedy(CATALOG_UNREADABLE.remedy)
        .detail(json!({ "detail": detail }))
    })?;
    if let Some(name) = inputs.value("name") {
        let entry = catalog
            .entries
            .iter()
            .find(|entry| entry.name == name)
            .ok_or_else(|| {
                Failure::invalid(
                    DIALOG_NOT_FOUND.code,
                    format!("`{name}` is not a catalogued dialog"),
                )
                .remedy(DIALOG_NOT_FOUND.remedy)
            })?;
        return Ok(json!({
            "version": catalog.version,
            "rule": RULE,
            "entry": entry.full(),
        }));
    }
    let action = inputs.value("action");
    let entries: Vec<Value> = catalog
        .entries
        .iter()
        .filter(|entry| action.is_none_or(|action| entry.action == action))
        .map(|entry| entry.summary())
        .collect();
    Ok(json!({
        "version": catalog.version,
        "rule": RULE,
        "count": entries.len(),
        "entries": entries,
    }))
}

pub fn render(data: &Value) -> String {
    let mut text = format!(
        "PLS-CADD dialog catalogue {}\n  {}\n",
        data["version"].as_str().unwrap_or(""),
        RULE
    );
    if let Some(entry) = data.get("entry") {
        text.push_str(&format!(
            "\n{}  {} (id {})\n  when   {}\n  title  {}\n  text   {}\n  note   {}\n",
            entry["name"].as_str().unwrap_or(""),
            entry["action"].as_str().unwrap_or(""),
            entry["control_id"],
            entry["when"].as_str().unwrap_or(""),
            entry["title_pattern"].as_str().unwrap_or(""),
            entry["text_pattern"].as_str().unwrap_or(""),
            entry["note"].as_str().unwrap_or(""),
        ));
        return text;
    }
    text.push('\n');
    for entry in data["entries"].as_array().into_iter().flatten() {
        text.push_str(&format!(
            "  {:<34} {:<13} {}\n",
            entry["name"].as_str().unwrap_or(""),
            entry["action"].as_str().unwrap_or(""),
            entry["when"].as_str().unwrap_or(""),
        ));
    }
    text
}
