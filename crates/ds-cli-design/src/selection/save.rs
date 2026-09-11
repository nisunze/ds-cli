//! `ds design selection save` — name a set of transformers, or replace one.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, ArgKind, Authority, Chapter, Command, Effect, Example, Execution,
};
use ds_cli_contract::{Context, Inputs};
use ds_client_core::{DesignSelectionRequest, DesignSelectionSave};
use serde_json::{Value, json};

use super::{ID_ARG, LANE};
use crate::MAX_SELECTION_MEMBERS;

const NAME_ARG: Arg = Arg {
    name: "name",
    kind: ArgKind::Value,
    value: "<text>",
    required: true,
    default: None,
    choices: &[],
    summary: "What the selection is for. The one field it cannot be without.",
};

const TRANSFORMERS_ARG: Arg = Arg {
    name: "transformers",
    kind: ArgKind::Value,
    value: "<names>",
    required: true,
    default: None,
    choices: &[],
    summary: "Comma-separated transformer names. These become stable identities.",
};

const SELECTION_ARG: Arg = Arg {
    name: "selection",
    kind: ArgKind::Value,
    value: "<selection-id>",
    required: false,
    default: None,
    choices: &[],
    summary: "Replace this existing selection's membership instead of creating one.",
};

const DESCRIPTION_ARG: Arg = Arg {
    name: "description",
    kind: ArgKind::Value,
    value: "<text>",
    required: false,
    default: None,
    choices: &[],
    summary: "Why this set exists. Empty is allowed and common.",
};

pub static COMMAND: Command = Command {
    id: "design.selection.save",
    path: &["design", "selection", "save"],
    contract: 1,
    summary: "Save a named selection of transformers, or replace one's members.",
    purpose: "\
Stores stable transformer identities under a name so the same set can be \
rediscovered, and assigned, later. Without --selection this creates a new \
saved selection; with it, the application reads the current version and \
replaces that selection's membership under it, so a concurrent edit is refused \
rather than overwritten. A name the project has no transformer for is refused \
at save time — a member that was already missing when it was saved would be \
noise on every later read.",
    chapter: Chapter::Design,
    effect: Effect::GlobalWrite,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[
        NAME_ARG,
        TRANSFORMERS_ARG,
        SELECTION_ARG,
        DESCRIPTION_ARG,
        ID_ARG,
        LANE,
    ],
    output: "The project, the `selection` id, its `name`, the committed `version`, and the member count.",
    examples: &[Example {
        command: "ds design selection save --name \"Week 32 review\" --transformers kigali_a,kigali_b --yes",
        note: "Without --yes dispatch refuses before the bridge is opened.",
        runnable: false,
    }],
    refusals: super::REFUSALS,
    reference: Some("docs/reference/design.md"),
    availability: ds_cli_auth::native_availability,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let lane = inputs.require("lane")?;
    let name = inputs.require("name")?;
    let transformers = crate::list_values(
        inputs.require("transformers")?,
        "transformers",
        MAX_SELECTION_MEMBERS,
    )?;
    let existing = inputs.value("selection");
    // Replacing an existing selection needs its current version, which is READ
    // here rather than accepted from the caller: `ds` must not be able to assert
    // a version it never observed.
    let expected_version = match existing {
        Some(selection) => Some(super::read_selection(lane, selection)?.1.selection.version),
        None => None,
    };
    let selection_id = match (existing, inputs.value("id")) {
        (Some(selection), _) => selection.to_owned(),
        (None, Some(pinned)) => pinned.to_owned(),
        (None, None) => super::mint_id("sel", name),
    };
    let members = transformers.len();
    let (project, answer) = super::ask(
        lane,
        &selection_id,
        &DesignSelectionRequest::Save(DesignSelectionSave {
            selection_id: selection_id.clone(),
            name: name.to_owned(),
            description: inputs.value("description").map(str::to_owned),
            member_ids: transformers,
            expected_version,
        }),
    )?;
    let head = super::saved_head(answer)?;
    Ok(json!({
        "project": project,
        "selection": head.selection_id,
        "name": head.name,
        "version": head.version,
        "members": members,
    }))
}

pub fn render(data: &Value) -> String {
    format!(
        "saved {} \"{}\" in {} · v{} · {}\n",
        data["selection"].as_str().unwrap_or("?"),
        data["name"].as_str().unwrap_or("?"),
        data["project"].as_str().unwrap_or("?"),
        data["version"].as_u64().unwrap_or(0),
        crate::plural(data["members"].as_u64().unwrap_or(0), "transformer"),
    )
}
