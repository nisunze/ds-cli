//! `ds map design pin` — read and manage the visible LV map Working set.
//!
//! The Working set (pinned transformers) is the map's curated read-only
//! context — a different state boundary from a governed Transformer Status
//! selection: `design.selection.save` snapshots a selection on the server,
//! while this command decides what the paired application's map paints as
//! `Working set N`. An explicit transformer family, a saved selection's
//! server-evaluated members, or both can be pinned; the application applies
//! its own soft limit and materializes the set exactly as its Pin verbs do.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, ArgKind, Authority, Chapter, Command, Effect, Example, Execution, Refusal,
};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Map, Value, json};

use crate::DESCRIPTOR_ARG;

const TRANSFORMERS_ARG: Arg = Arg {
    name: "transformer",
    kind: ArgKind::Repeated,
    value: "<name>",
    required: false,
    default: None,
    choices: &[],
    summary: "Exact transformer to pin or unpin. Repeat for a family.",
};

const SELECTION_ARG: Arg = Arg {
    name: "selection",
    kind: ArgKind::Value,
    value: "<id>",
    required: false,
    default: None,
    choices: &[],
    summary: "Saved Transformer Status selection whose present members to load.",
};

const MODE_ARG: Arg = Arg {
    name: "mode",
    kind: ArgKind::Value,
    value: "<mode>",
    required: false,
    default: Some("set"),
    choices: &["read", "set", "add", "remove", "unpin", "load", "clear"],
    summary: "read inspects; load replaces from one selection; set/add pin; remove/unpin adjust; clear empties.",
};

pub static COMMAND: Command = Command {
    id: "map.design.pin",
    path: &["map", "design", "pin"],
    contract: 2,
    summary: "Read, load, pin, unpin, or clear the visible map Working set.",
    purpose: "\
Reads and manages the paired application's LV map Working set — the pinned read-only \
transformer context the map paints. Pass exact transformer names, a saved \
Transformer Status selection (its server-evaluated present members are \
pinned; missing members are reported, never guessed), or both. The Working \
set is local view state: nothing is staged into a room and nothing is \
persisted to the project.",
    chapter: Chapter::Design,
    effect: Effect::LocalUi,
    authority: Authority::Project,
    execution: Execution::Sync,
    args: &[TRANSFORMERS_ARG, SELECTION_ARG, MODE_ARG, DESCRIPTOR_ARG],
    output: "\
The project, the applied mode, the resulting pinned names and count, and — \
when a selection was loaded — any members missing from the project. \
`staged` and `persisted` are explicitly false: the Working set is view \
state.",
    examples: &[
        Example {
            command: "ds map design pin --mode read --output json",
            note: "Reads the current Working set without loading or changing it.",
            runnable: false,
        },
        Example {
            command: "ds map design pin --transformer agasharu --transformer gitega --output json",
            note: "Replaces the Working set with exactly these two transformers and paints them.",
            runnable: false,
        },
        Example {
            command: "ds map design pin --selection phase1-review --mode load --output json",
            note: "Replaces the Working set with a saved selection's present members.",
            runnable: false,
        },
        Example {
            command: "ds map design pin --transformer agasharu --mode unpin --output json",
            note: "Unpins one exact transformer without disturbing the remaining Working set.",
            runnable: false,
        },
        Example {
            command: "ds map design pin --mode clear --output json",
            note: "Empties the Working set and hides the pinned context.",
            runnable: false,
        },
    ],
    refusals: &[
        crate::NOT_PAIRED,
        crate::AMBIGUOUS,
        crate::UNREACHABLE,
        crate::PAIRING_REJECTED,
        crate::SIGNED_OUT,
        Refusal {
            code: "auth_context_mismatch",
            when: "the protected headless provider and paired map use different lane, account, audience, or project identity",
            remedy: "use the matching Desktop lane, account and project, then verify `ds desktop status` before retrying",
        },
        Refusal {
            code: "desktop_refused",
            when: "a named transformer or the selection does not exist in the active project",
            remedy: "check names with `ds map design list` and selections with `ds design selection list`, then retry",
        },
        Refusal {
            code: "invalid_mode",
            when: "--mode has an invalid target shape: read/clear take none, load takes one selection, or unpin takes transformer names",
            remedy: "use read or clear alone; load with --selection; unpin with one or more --transformer values",
        },
        crate::UNSUPPORTED,
        crate::UNREADABLE,
    ],
    reference: Some("docs/reference/map.md"),
    availability: crate::paired_availability,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let transformers = inputs.repeated("transformer");
    let selection = inputs.value("selection");
    let mode = inputs.value("mode").unwrap_or("set");
    if !matches!(
        mode,
        "read" | "set" | "add" | "remove" | "unpin" | "load" | "clear"
    ) {
        return Err(Failure::invalid(
            "invalid_mode",
            "--mode must be read, set, add, remove, unpin, load, or clear",
        )
        .remedy("read/clear take no target; load takes --selection; unpin takes --transformer"));
    }
    if matches!(mode, "read" | "clear") && (!transformers.is_empty() || selection.is_some()) {
        return Err(Failure::invalid(
            "invalid_mode",
            format!("--mode {mode} does not take --transformer or --selection"),
        )
        .remedy(format!("run `ds map design pin --mode {mode}` alone")));
    }
    if mode == "load" && (selection.is_none() || !transformers.is_empty()) {
        return Err(Failure::invalid(
            "invalid_mode",
            "--mode load takes exactly one --selection and no --transformer",
        )
        .remedy("run `ds map design pin --mode load --selection <id>`"));
    }
    if mode == "unpin" && (transformers.is_empty() || selection.is_some()) {
        return Err(Failure::invalid(
            "invalid_mode",
            "--mode unpin takes one or more --transformer values and no --selection",
        )
        .remedy("run `ds map design pin --mode unpin --transformer <name>`"));
    }
    if !matches!(mode, "read" | "clear") && transformers.is_empty() && selection.is_none() {
        return Err(Failure::invalid(
            "invalid_mode",
            "pass at least one --transformer, a --selection, --mode read, or --mode clear",
        )
        .remedy("name what to pin, read the Working set, or clear it explicitly"));
    }

    let mut arguments = Map::new();
    arguments.insert("mode".into(), json!(mode));
    if !transformers.is_empty() {
        arguments.insert("transformers".into(), json!(transformers));
    }
    if let Some(selection) = selection {
        arguments.insert("selection".into(), json!(selection));
    }

    let descriptor = crate::paired(inputs.value("desktop-descriptor"))?;
    let result = crate::invoke(
        &descriptor,
        &crate::DESIGN_PIN,
        Value::Object(arguments),
        crate::DESIGN_STAGE_TIMEOUT,
    )
    .map_err(crate::classify_design_failure)?;

    Ok(json!({
        "project": result["project"],
        "mode": result["mode"],
        "pinned_count": result["pinnedCount"].as_u64().unwrap_or(0),
        "pinned": result["pinned"],
        "missing_selection_members": result
            .get("missingSelectionMembers")
            .cloned()
            .unwrap_or(Value::Array(Vec::new())),
        "staged": false,
        "persisted": false,
    }))
}

pub fn render(data: &Value) -> String {
    let count = data["pinned_count"].as_u64().unwrap_or(0);
    let mut out = format!(
        "Working set: {count} transformer(s) pinned in {}\n",
        data["project"].as_str().unwrap_or("?")
    );
    if let Some(rows) = data["pinned"].as_array() {
        for row in rows {
            out.push_str(&format!("  {}\n", row.as_str().unwrap_or("?")));
        }
    }
    if let Some(missing) = data["missing_selection_members"].as_array()
        && !missing.is_empty()
    {
        out.push_str(&format!(
            "{} selection member(s) missing from the project were not pinned.\n",
            missing.len()
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use ds_cli_contract::args::parse;
    use ds_cli_contract::{Format, Output};

    fn inputs(arguments: &[&str]) -> Inputs {
        parse(
            &COMMAND,
            &arguments
                .iter()
                .map(|value| (*value).to_owned())
                .collect::<Vec<_>>(),
        )
        .expect("valid pin arguments")
    }

    fn context() -> Context {
        Context {
            confirmed: false,
            output: Output {
                format: Format::Json,
                pretty: false,
                color: false,
            },
        }
    }

    #[test]
    fn clear_refuses_a_transformer_before_pairing() {
        let failure = run(
            &inputs(&["--mode", "clear", "--transformer", "agasharu"]),
            &context(),
        )
        .expect_err("clear plus a transformer must refuse");
        assert_eq!(failure.code(), "invalid_mode");
    }

    #[test]
    fn read_refuses_a_target_before_pairing() {
        let failure = run(
            &inputs(&["--mode", "read", "--transformer", "agasharu"]),
            &context(),
        )
        .expect_err("read plus a transformer must refuse");
        assert_eq!(failure.code(), "invalid_mode");
    }

    #[test]
    fn load_requires_only_a_selection_before_pairing() {
        for arguments in [
            vec!["--mode", "load"],
            vec![
                "--mode",
                "load",
                "--selection",
                "phase-1",
                "--transformer",
                "agasharu",
            ],
        ] {
            let failure =
                run(&inputs(&arguments), &context()).expect_err("invalid load must refuse");
            assert_eq!(failure.code(), "invalid_mode");
        }
    }

    #[test]
    fn unpin_requires_only_transformers_before_pairing() {
        for arguments in [
            vec!["--mode", "unpin"],
            vec!["--mode", "unpin", "--selection", "phase-1"],
        ] {
            let failure =
                run(&inputs(&arguments), &context()).expect_err("invalid unpin must refuse");
            assert_eq!(failure.code(), "invalid_mode");
        }
    }

    #[test]
    fn set_refuses_an_empty_target_before_pairing() {
        let failure = run(&inputs(&[]), &context()).expect_err("an empty set must refuse");
        assert_eq!(failure.code(), "invalid_mode");
    }

    #[test]
    fn bridge_operation_names_only_the_published_arguments() {
        assert_eq!(
            crate::DESIGN_PIN.arguments,
            &["transformers", "selection", "mode"]
        );
    }

    #[test]
    fn human_receipt_keeps_missing_selection_members_visible() {
        let rendered = render(&json!({
            "project": "project-a",
            "pinned_count": 2,
            "pinned": ["agasharu", "gitega"],
            "missing_selection_members": ["retired-transformer"],
        }));
        assert!(rendered.contains("2 transformer(s) pinned"));
        assert!(rendered.contains("1 selection member(s) missing"));
    }
}
