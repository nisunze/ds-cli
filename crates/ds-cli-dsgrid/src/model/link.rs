//! `ds dsgrid model link` — pin a working copy to the live PLS-CADD
//! workspace it was converted from.
//!
//! A working copy imported from a PLS-CADD folder knows the bytes it came
//! from (the package preserves the original member tree) but not where they
//! live. The link records where: the folder, the exchange digest of its
//! member tree — the same digest `ds dsgrid-exchange inspect` prints — the
//! PLS-CADD program version and every member family's version. With it,
//! `ds dsgrid-exchange sync` writes edits back INTO that folder instead of
//! into a new one, and refuses if the folder's bytes moved since.
//!
//! The folder must digest to the workspace the package was imported from.
//! A link to some other folder — a sibling copy with one file changed, an
//! older revision — is refused by name, because a sync into it would write
//! edits computed against a different baseline.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, ArgKind, Authority, Availability, Chapter, Command, Effect, Example, Execution, Refusal,
    Requires,
};
use ds_cli_contract::{Context, Inputs};
use ds_command_kernel::local_models::Op;
use serde_json::{Value, json};

use crate::model::{pls_source, workspace};

const MODEL_ARG: Arg = Arg {
    name: "model",
    kind: ArgKind::Value,
    value: "<model-id>",
    required: true,
    default: None,
    choices: &[],
    summary: "The working copy to link, by the id `ds dsgrid model list` reports.",
};

const WORKSPACE_ARG: Arg = Arg {
    name: "workspace",
    kind: ArgKind::Value,
    value: "<folder>",
    required: true,
    default: None,
    choices: &[],
    summary: "The PLS-CADD workspace folder (the one holding the .don) this copy was converted from.",
};

const LINK_OWN: [Refusal; 5] = [
    pls_source::WORKSPACE_NOT_FOUND,
    pls_source::WORKSPACE_NOT_THIS_PACKAGE,
    pls_source::MODEL_NOT_FROM_PLS,
    crate::folder::TOO_LARGE,
    crate::folder::UNREADABLE,
];
const REFUSALS: &[Refusal; LINK_OWN.len() + workspace::REFUSALS.len()] = &refusals();
const fn refusals() -> [Refusal; LINK_OWN.len() + workspace::REFUSALS.len()] {
    let mut all = [pls_source::WORKSPACE_NOT_FOUND; LINK_OWN.len() + workspace::REFUSALS.len()];
    let mut index = 0;
    while index < LINK_OWN.len() {
        all[index] = LINK_OWN[index];
        index += 1;
    }
    let mut shared = 0;
    while shared < workspace::REFUSALS.len() {
        all[LINK_OWN.len() + shared] = workspace::REFUSALS[shared];
        shared += 1;
    }
    all
}

pub static COMMAND: Command = Command {
    id: "dsgrid.model.link",
    path: &["dsgrid", "model", "link"],
    contract: 1,
    summary: "Pin a working copy to the live PLS-CADD workspace it came from.",
    purpose: "\
Records on this machine's catalogue row which PLS-CADD workspace folder a \
working copy was converted from: the folder, the exchange digest of its \
member tree (the digest `ds dsgrid-exchange inspect` prints), the PLS-CADD \
program version and each native member family's version (DON 57, CRI 94, \
FEA 15 …). The folder must digest to the workspace the package preserved at \
import; any other folder is refused. With the link, `ds dsgrid-exchange \
sync` writes DS edits back into that folder in place. Relinking replaces the \
previous link. Nothing in the workspace is read for engineering and nothing \
in it is written.",
    chapter: Chapter::GridModel,
    effect: Effect::LocalFileWrite,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[
        MODEL_ARG,
        WORKSPACE_ARG,
        workspace::LANE_ARG,
        workspace::ACCOUNT_ARG,
    ],
    output: "\
`status: linked`, the `model` row with its `pls_source` {path, digest, \
pls_version, member_versions, member_count, linked_at}, `replaced` when a \
previous link was overwritten, and a `streamed_volume` warning when the \
folder is on a streamed or network drive.",
    examples: &[Example {
        command: "ds dsgrid model link --model local-5ff16cd0a3d6416b --workspace \"/srv/pls/Nyamagabe\" --account <uid> --output json",
        note: "Then `ds dsgrid-exchange sync --model local-5ff16cd0a3d6416b --dry-run`.",
        runnable: false,
    }],
    refusals: REFUSALS,
    reference: Some("docs/reference/dsgrid.md"),
    search: &["pls-cadd", "workspace", "pin", "provenance", "connect"],
    requires: Requires::Server,
    availability: || Availability::Available,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let id = inputs.require("model")?.trim().to_owned();
    let opened = pls_source::open(inputs, &id)?;
    let source = pls_source::pls_source(&id, &opened.package)?;
    let workspace_read = pls_source::read_workspace(inputs.require("workspace")?)?;
    if workspace_read.digest != source.origin_digest {
        return Err(Failure::conflict(
            pls_source::WORKSPACE_NOT_THIS_PACKAGE.code,
            format!(
                "`{}` digests to {} but `{id}` was imported from a workspace digesting to {}",
                workspace_read.path.display(),
                workspace_read.digest,
                source.origin_digest
            ),
        )
        .remedy(pls_source::WORKSPACE_NOT_THIS_PACKAGE.remedy)
        .detail(json!({
            "workspace_digest": workspace_read.digest,
            "package_origin_digest": source.origin_digest,
            "workspace_members": workspace_read.members.len(),
            "package_members": source.original_members.len(),
        })));
    }
    let replaced = opened.row.pls_source.clone();
    let link = pls_source::link_for(&workspace_read);
    let outcome = workspace::execute(
        inputs,
        Op::Link {
            id: id.clone(),
            pls_source: link,
        },
        None,
    )?;
    let linked = outcome
        .model
        .as_ref()
        .ok_or_else(|| Failure::internal("local_model_store_unavailable", "nothing was linked"))?;
    let mut warnings = Vec::new();
    if let Some(hint) = &workspace_read.streamed_volume {
        warnings.push(format!("streamed_volume: {hint}"));
    }
    Ok(json!({
        "status": "linked",
        "model": workspace::row(linked, outcome.catalogue.active.as_deref()),
        "replaced": pls_source::link_json(replaced.as_ref()),
        "warnings": warnings,
    }))
}

pub fn render(data: &Value) -> String {
    let model = &data["model"];
    let mut out = format!(
        "linked {} · {}\n",
        model["model"].as_str().unwrap_or("?"),
        model["name"].as_str().unwrap_or(""),
    );
    let link = &model["pls_source"];
    out.push_str(&format!(
        "  workspace  {}\n  PLS-CADD   {} · {} members\n  digest     {}\n",
        link["path"].as_str().unwrap_or("?"),
        link["pls_version"].as_str().unwrap_or("?"),
        link["member_count"],
        link["digest"].as_str().unwrap_or("?"),
    ));
    if let Some(versions) = link["member_versions"].as_object() {
        let line = versions
            .iter()
            .map(|(family, version)| format!("{family} {}", version.as_str().unwrap_or("?")))
            .collect::<Vec<_>>()
            .join(" · ");
        out.push_str(&format!("  members    {line}\n"));
    }
    if !data["replaced"].is_null() {
        out.push_str(&format!(
            "  replaced   {}\n",
            data["replaced"]["path"].as_str().unwrap_or("?")
        ));
    }
    for warning in data["warnings"].as_array().into_iter().flatten() {
        out.push_str(&format!(
            "  warning    {}\n",
            warning.as_str().unwrap_or("")
        ));
    }
    out
}
