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
    AMBIGUOUS, DESCRIPTOR_ARG, LOCAL_TIMEOUT, NOT_PAIRED, PAIRING_REJECTED, REFUSED, UNREACHABLE,
    UNREADABLE, UNSUPPORTED, paired_availability, workspace,
};

/// What the application is asked: open these package bytes (by path, never
/// over the bridge) under this id and name, then occupy Profile on one
/// alignment. `alignment` is absent when the caller left the choice to the
/// model's first alignment.
pub const PROFILE_OPEN: BridgeOp = BridgeOp {
    operation: "dsgrid.profile.open",
    arguments: &["model", "path", "name", "alignment"],
};

const MODEL_ARG: Arg = Arg {
    name: "model",
    kind: ArgKind::Value,
    value: "<model-id>",
    required: true,
    default: None,
    choices: &[],
    summary: "The working copy to show, by the id `ds dsgrid model list` reports.",
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

const ALIGNMENT_NOT_FOUND: Refusal = Refusal {
    code: "alignment_not_found",
    when: "the model carries no alignment by that id, or no alignment at all",
    remedy: "run `ds dsgrid inspect --model <path> --include tables` and pass one alignment id",
};

const OWN: [Refusal; 8] = [
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
    contract: 1,
    summary: "Show one working copy in the paired DS GridDesign's Profile view.",
    purpose: "\
Opens this machine's working copy in the running DS GridDesign and occupies \
its Profile view with it, focused on one alignment. The package is read by \
the application from the catalogue's own file — no bytes cross the bridge — \
and it is opened under the copy's id, so what `ds dsgrid model show` names \
and what the window shows are one model. Reopening the copy the application \
already holds is a focus change, never a second session. The session is not \
added to the application's catalogue; the operator's checkpoint does that.",
    chapter: Chapter::GridModel,
    effect: Effect::LocalUi,
    authority: Authority::DesktopPairing,
    execution: Execution::Sync,
    args: &[
        MODEL_ARG,
        ALIGNMENT_ARG,
        workspace::ACCOUNT_ARG,
        workspace::LANE_ARG,
        DESCRIPTOR_ARG,
    ],
    output: "The copy, its package path, whether the application had it open already, the alignment focused and the alignments it carries.",
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
    ],
    refusals: REFUSALS,
    reference: Some("docs/reference/dsgrid.md"),
    search: &["working copy", "alignment", "desktop"],
    // The two halves of one fact: this command asks the application for its
    // answer, so it declares the window and hands over the paired availability.
    requires: Requires::Window,
    availability: paired_availability,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let id = inputs.require("model")?.trim().to_owned();
    let located = workspace::locate(inputs, &id)?;
    let path = located.path.display().to_string();
    let mut arguments = json!({
        "model": located.row.id,
        "path": path,
        "name": located.row.display_name,
    });
    if let Some(alignment) = inputs.value("alignment").map(str::trim).filter(|a| !a.is_empty()) {
        arguments["alignment"] = json!(alignment);
    }
    let descriptor = crate::model::paired(inputs.value("desktop-descriptor"))?;
    let result = crate::model::invoke(&descriptor, &PROFILE_OPEN, arguments, LOCAL_TIMEOUT)
        .map_err(classify)?;
    receipt(&located.row.id, &result)
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
    if returned != id || !opened || alignment.is_none() {
        return Err(Failure::failed(
            "desktop_unreadable",
            "the application did not confirm Profile occupied by the requested working copy",
        )
        .detail(json!({ "reply": result }))
        .remedy(UNREADABLE.remedy));
    }
    Ok(json!({
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
    }))
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
