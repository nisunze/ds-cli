//! `ds dsgrid profile open` — show one of this machine's working copies in
//! the paired DS GridDesign's Profile view.
//!
//! The working copy is a fact about this machine (`ds dsgrid model list`);
//! the application holds its own open sessions. This is the one door
//! between them: `ds` resolves the copy's package on disk and the
//! application opens those bytes under the copy's own id, occupies Profile
//! with it and focuses one alignment — the one named, or the model's first.
//! Nothing is copied into the application's catalogue: a checkpoint by the
//! operator is what makes the session durable there, exactly as for an
//! imported file.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, ArgKind, Authority, Chapter, Command, Effect, Example, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Inputs};
use ds_cli_desktop::ops::BridgeOp;
use serde_json::{Value, json};

use crate::model::{
    AMBIGUOUS, DESCRIPTOR_ARG, LOCAL_TIMEOUT, NOT_PAIRED, PAIRING_REJECTED, REFUSED, TARGET_ARG,
    UNREACHABLE, UNREADABLE, UNSUPPORTED, paired_availability, workspace,
};

/// What the application is asked: open these package bytes (by path, never
/// over the bridge) under this id and name, then occupy Profile on one
/// alignment. `alignment` is absent when the caller left the choice to the
/// model's first alignment.
pub const PROFILE_OPEN: BridgeOp = BridgeOp {
    operation: "dsgrid.profile.open",
    arguments: &[
        "model",
        "path",
        "name",
        "alignment",
        "checkpoint_out",
        "expect_revision",
        "replace_temporary_models",
        "live",
        "expect_history",
    ],
};

const MODEL_ARG: Arg = Arg {
    name: "model",
    kind: ArgKind::Value,
    value: "<model-id>",
    required: true,
    default: None,
    choices: &[],
    summary: "Working-copy id, or the open Profile model id with --live.",
};

const ALIGNMENT_ARG: Arg = Arg {
    name: "alignment",
    kind: ArgKind::Value,
    value: "<alignment-id>",
    required: false,
    default: None,
    choices: &[],
    summary: "The alignment to focus; omitted, the model's first alignment.",
};

const REPLACE_TEMPORARY_MODELS_ARG: Arg = Arg {
    name: "replace-temporary-models",
    kind: ArgKind::Switch,
    value: "",
    required: false,
    default: None,
    choices: &[],
    summary: "Remove temporary models and groups before opening this working copy.",
};

const ALIGNMENT_NOT_FOUND: Refusal = Refusal {
    code: "alignment_not_found",
    when: "the model carries no alignment by that id, or no alignment at all",
    remedy: "run `ds dsgrid inspect --model <path> --include tables` and pass one alignment id",
};

const OWN: [Refusal; 16] = [
    Refusal {
        code: "checkpoint_history_invalid",
        when: "--live lacks checkpoint output or full native history, mixes opening controls, or history JSON is invalid",
        remedy: "use --live --checkpoint-out with --expect-history from map profile view; omit opening controls",
    },
    Refusal {
        code: "history_conflict",
        when: "the open model's full history changed before checkpoint admission",
        remedy: "read map profile view and review the current history before capturing",
    },
    Refusal {
        code: "checkpoint_output_invalid",
        when: "checkpoint output is not an absolute .dsgrid path, or --expect-revision is given without it",
        remedy: "pass --checkpoint-out with a new absolute .dsgrid path",
    },
    Refusal {
        code: "revision_conflict",
        when: "the captured live head differs from --expect-revision",
        remedy: "inspect the live revision and choose again",
    },
    Refusal {
        code: "checkpoint_failed",
        when: "the desktop could not capture or write the exact live checkpoint",
        remedy: "read the desktop refusal; preserve the live session and choose a new writable output path",
    },
    Refusal {
        code: "output_exists",
        when: "the checkpoint destination exists",
        remedy: "choose a new checkpoint filename",
    },
    Refusal {
        code: "output_parent_missing",
        when: "the checkpoint output parent is missing",
        remedy: "create the intended output directory",
    },
    Refusal {
        code: "output_unwritable",
        when: "the checkpoint output is not a writable destination",
        remedy: "choose a writable new output path",
    },
    NOT_PAIRED,
    AMBIGUOUS,
    UNREACHABLE,
    PAIRING_REJECTED,
    UNSUPPORTED,
    UNREADABLE,
    REFUSED,
    ALIGNMENT_NOT_FOUND,
];
const REFUSALS: &[Refusal; OWN.len() + workspace::REFUSALS.len()] = &refusals();
const fn refusals() -> [Refusal; OWN.len() + workspace::REFUSALS.len()] {
    let mut all = [OWN[0]; OWN.len() + workspace::REFUSALS.len()];
    let mut index = 0;
    while index < OWN.len() {
        all[index] = OWN[index];
        index += 1;
    }
    let mut shared = 0;
    while shared < workspace::REFUSALS.len() {
        all[OWN.len() + shared] = workspace::REFUSALS[shared];
        shared += 1;
    }
    all
}

pub static COMMAND: Command = Command {
    id: "dsgrid.profile.open",
    path: &["dsgrid", "profile", "open"],
    contract: 2,
    summary: "Open a working copy or checkpoint a live Desktop Profile model.",
    purpose: "Open a machine working copy in Profile, optionally focusing an alignment and writing a new checkpoint. Reopening retains its live session. --live instead captures the already-open Profile model without catalogue lookup, import or focus changes; requires --checkpoint-out and --expect-history from map profile view. Native full-history admission rejects Undo/Redo ABA. Capture never saves or adopts the working model; import the new package explicitly to create an independent copy. No model bytes cross the bridge. --replace-temporary-models removes temporary groups/models only when explicitly requested during ordinary opening.",
    chapter: Chapter::GridModel,
    effect: Effect::LocalFileWrite,
    authority: Authority::DesktopPairing,
    execution: Execution::Sync,
    args: &[
        MODEL_ARG,
        ALIGNMENT_ARG,
        Arg::value(
            "checkpoint-out",
            "<absolute-path>",
            "Optional new absolute .dsgrid file for the exact live model; never overwritten.",
        ),
        Arg::value(
            "expect-revision",
            "<rev>",
            "Optional expected live authored head; requires --checkpoint-out.",
        ),
        REPLACE_TEMPORARY_MODELS_ARG,
        Arg::switch(
            "live",
            "Capture the already-open Profile model, including models outside the CLI catalogue; requires output and full history.",
        ),
        Arg::value(
            "expect-history",
            "<native-json>",
            "Complete native history from map profile view (2 KiB); required with --live.",
        ),
        Arg::value(
            "account",
            "<uid>",
            "Required for ordinary opening: account owning the machine catalogue. Live capture uses the paired Desktop identity.",
        ),
        workspace::LANE_ARG,
        TARGET_ARG,
        DESCRIPTOR_ARG,
    ],
    output: "Model, package path, live revision and Profile state; checkpoint receipt names the captured revision, full history with --live, persisted file path, SHA-256 and byte length. Live capture leaves alignment/focus unchanged. Explicit replacement reports removed temporary groups/models.",
    examples: &[
        Example {
            command: "ds dsgrid profile open --model local-b1b2d3b9e6ab4959",
            note: "Occupies Profile with the working copy on its first alignment.",
            runnable: false,
        },
        Example {
            command: "ds dsgrid profile open --model local-b1b2d3b9e6ab4959 --alignment al-0007 --output json",
            note: "Focuses one named alignment and returns the receipt.",
            runnable: false,
        },
        Example {
            command: "ds dsgrid profile open --model local-b1b2d3b9e6ab4959 --replace-temporary-models --output json",
            note: "Removes browser-local temporary groups and models before opening; the receipt counts them.",
            runnable: false,
        },
    ],
    refusals: REFUSALS,
    reference: Some("docs/reference/dsgrid.md"),
    search: &[
        "working copy",
        "alignment",
        "desktop",
        "checkpoint",
        "clone",
    ],
    // The two halves of one fact: this command asks the application for its
    // answer, so it declares the window and hands over the paired availability.
    requires: Requires::Window,
    availability: paired_availability,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let checkpoint_out = checkpoint_output(inputs)?;
    let id = inputs.require("model")?.trim().to_owned();
    let mut arguments = if inputs.switch("live") {
        live_arguments(inputs)?
    } else {
        if inputs.value("expect-history").is_some() {
            return Err(history_invalid());
        }
        let located = workspace::locate(inputs, &id)?;
        json!({"model":located.row.id,"path":located.path.display().to_string(),"name":located.row.display_name})
    };
    if let Some(alignment) = inputs
        .value("alignment")
        .map(str::trim)
        .filter(|a| !a.is_empty())
    {
        arguments["alignment"] = json!(alignment);
    }
    if let Some(path) = checkpoint_out {
        arguments["checkpoint_out"] = json!(path);
    }
    if let Some(revision) = inputs.value("expect-revision") {
        arguments["expect_revision"] = json!(revision);
    }
    if inputs.switch("replace-temporary-models") {
        arguments["replace_temporary_models"] = json!(true);
    }
    let descriptor = crate::model::paired(inputs.value("desktop-descriptor"))?;
    let result = crate::model::invoke(&descriptor, &PROFILE_OPEN, arguments, LOCAL_TIMEOUT)
        .map_err(classify)?;
    let receipt = receipt(&id, &result)?;
    if checkpoint_out.is_some() && receipt["checkpoint"]["persisted"] != true {
        return Err(Failure::failed(
            "desktop_unreadable",
            "the application did not confirm the requested live checkpoint",
        )
        .remedy(UNREADABLE.remedy));
    }
    Ok(receipt)
}

fn history_invalid() -> Failure {
    Failure::invalid("checkpoint_history_invalid", "Live checkpoint requires bounded native history and a new output, without opening controls.")
        .remedy("use --live --checkpoint-out with --expect-history from map profile view; omit opening controls")
}

fn live_arguments(inputs: &Inputs) -> Result<Value, Failure> {
    if inputs.value("checkpoint-out").is_none()
        || inputs.value("alignment").is_some()
        || inputs.value("expect-revision").is_some()
        || inputs.switch("replace-temporary-models")
    {
        return Err(history_invalid());
    }
    let raw = inputs.value("expect-history").ok_or_else(history_invalid)?;
    if raw.len() > 2048 {
        return Err(history_invalid());
    }
    let history: ds_grid_engine::session::SessionHistoryState =
        serde_json::from_str(raw).map_err(|_| history_invalid())?;
    let id = inputs.require("model")?;
    if id.trim().is_empty() || id.len() > 200 || id.chars().any(char::is_control) {
        return Err(history_invalid());
    }
    Ok(json!({"model":id.trim(),"live":true,"expect_history":history}))
}

fn checkpoint_output(inputs: &Inputs) -> Result<Option<&str>, Failure> {
    let out = inputs.value("checkpoint-out");
    let invalid = || {
        Failure::invalid(
            "checkpoint_output_invalid",
            "checkpoint requires a new absolute .dsgrid path",
        )
        .remedy("pass --checkpoint-out with a new absolute .dsgrid path")
    };
    if inputs.value("expect-revision").is_some() && out.is_none() {
        return Err(invalid());
    }
    if let Some(path) = out {
        let path_value = std::path::Path::new(path);
        if !path_value.is_absolute()
            || path_value.extension().and_then(|v| v.to_str()) != Some("dsgrid")
        {
            return Err(invalid());
        }
        crate::apply::validate_output_path(path)?;
    }
    Ok(out)
}

/// The application's own refusal for a missing alignment is named here so a
/// caller branches on a code, not on prose.
fn classify(failure: Failure) -> Failure {
    let failure = crate::model::classify(failure);
    if failure.code() != "desktop_refused" {
        return failure;
    }
    let detail = failure
        .detail_value()
        .and_then(|detail| detail["detail"].as_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    if detail.contains("checkpoint_history_conflict:") {
        return Failure::invalid("history_conflict", detail)
            .remedy("read map profile view and review the current history before capturing");
    }
    if detail.contains("checkpoint_revision_conflict:") {
        return Failure::invalid("revision_conflict", detail)
            .remedy("inspect the live revision and choose again");
    }
    if detail.contains("checkpoint_failed:") {
        return Failure::failed("checkpoint_failed", detail)
            .remedy("preserve the live session and choose a new writable output path");
    }
    if detail.contains("alignment") && detail.contains("not") {
        return Failure::invalid(ALIGNMENT_NOT_FOUND.code, detail)
            .remedy(ALIGNMENT_NOT_FOUND.remedy);
    }
    failure
}

fn receipt(id: &str, result: &Value) -> Result<Value, Failure> {
    let returned = result["model"].as_str().unwrap_or_default();
    let opened = result["profile_open"].as_bool().unwrap_or(false);
    let alignment = result["alignment"].as_str();
    let live_checkpoint =
        result["live_checkpoint"] == true && result["checkpoint"]["persisted"] == true;
    if returned != id || !opened || (alignment.is_none() && !live_checkpoint) {
        return Err(Failure::failed(
            "desktop_unreadable",
            "the application did not confirm Profile occupied by the requested working copy",
        )
        .detail(json!({ "reply": result }))
        .remedy(UNREADABLE.remedy));
    }
    let mut receipt = json!({
        "model": id,
        "path": result["path"],
        "name": result["name"],
        "already_open": result["already_open"].as_bool().unwrap_or(false),
        "profile_open": true,
        "alignment": alignment,
        "alignment_label": result["alignment_label"],
        "alignments": result["alignments"],
        "revision": result["revision"],
        // What the window holds after the open: the session the Profile
        // reads and the workspace state, so a blank Profile is diagnosable
        // from the receipt alone.
        "session": result["session"],
        "workspace": result["workspace"],
        "runtime_errors": result["runtime_errors"],
        "checkpoint": result["checkpoint"],
        "live_checkpoint": live_checkpoint,
    });
    if let Some(removed) = result.get("temporary_models_removed") {
        receipt["temporary_models_removed"] = removed.clone();
    }
    Ok(receipt)
}

pub fn render(data: &Value) -> String {
    format!(
        "profile  {} · {}\n  alignment  {}{}\n  package    {}{}\n",
        data["model"].as_str().unwrap_or("?"),
        data["name"].as_str().unwrap_or(""),
        data["alignment"].as_str().unwrap_or("?"),
        data["alignment_label"]
            .as_str()
            .filter(|label| !label.is_empty())
            .map(|label| format!(" ({label})"))
            .unwrap_or_default(),
        data["path"].as_str().unwrap_or("?"),
        if data["already_open"].as_bool().unwrap_or(false) {
            "  (was open; focused)"
        } else {
            ""
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn live_capture_validates_native_history_without_catalogue_or_pairing() {
        let history = r#"{"model_revision":"rev:head","initial_revision":"rev:head","undo_depth":0,"redo_depth":1,"history_pin":"pin"}"#;
        let parse = |extra: &[&str]| {
            let mut args = vec![
                "--model",
                "browser-only",
                "--live",
                "--checkpoint-out",
                "/unused/new.dsgrid",
            ];
            args.extend_from_slice(extra);
            ds_cli_contract::args::parse(
                &COMMAND,
                &args.iter().map(|v| (*v).into()).collect::<Vec<String>>(),
            )
            .unwrap()
        };
        let inputs = parse(&["--expect-history", history]);
        let arguments = live_arguments(&inputs).unwrap();
        assert_eq!(arguments["model"], "browser-only");
        assert_eq!(arguments["expect_history"]["redo_depth"], 1);
        assert!(arguments.get("path").is_none());
        assert!(live_arguments(&parse(&[])).is_err());
        assert!(
            live_arguments(&parse(&[
                "--expect-history",
                history,
                "--alignment",
                "al-1"
            ]))
            .is_err()
        );
        assert!(
            live_arguments(&parse(&["--expect-history", r#"{"project":"untrusted"}"#])).is_err()
        );
        let data = receipt("browser-only", &json!({"model":"browser-only","profile_open":true,"live_checkpoint":true,"checkpoint":{"persisted":true}})).unwrap();
        assert!(data["alignment"].is_null());
    }

    #[test]
    fn profile_open_receipt_keeps_temporary_removal_counts() {
        let reply = json!({
            "model": "local-1",
            "profile_open": true,
            "alignment": "al-1",
            "temporary_models_removed": {"groups": 1, "models": 2}
        });
        let data = receipt("local-1", &reply).expect("valid profile open");
        assert_eq!(
            data["temporary_models_removed"],
            json!({"groups": 1, "models": 2})
        );
        let mut no_replacement = reply;
        no_replacement
            .as_object_mut()
            .unwrap()
            .remove("temporary_models_removed");
        let data = receipt("local-1", &no_replacement).expect("valid profile open");
        assert!(data.get("temporary_models_removed").is_none());
    }
}
