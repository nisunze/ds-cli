//! `ds design attachment` — immutable, versioned files on a design object.
//!
//! ```text
//!   list | list-project | show | versions → publish | download | set-latest | retire
//! ```
//!
//! An attachment is OPAQUE: a PLS-CADD `.bak`, a native workspace, a report, a
//! photo, a client deliverable. Nothing here parses it. Publishing a revision
//! never mutates earlier bytes — each revision owns its own object, its own
//! server-verified digest and its own storage generation — and a download comes
//! back signed by ds-brain and pinned to that generation, so `ds` never composes
//! a storage URL.
//!
//! On a DS Grid model the version pin is the content revision a submission
//! was published as; the delivered `.bak` binds there, with the model digest
//! beside it, and `versions` answers which model versions carry one file.
//! Every action ds-brain serves on this route has one verb here, and every
//! action the kernel's attachment policy offers names its verb
//! (`ds_command_kernel::design_attachments::ACTION_COMMANDS`).

pub mod download;
pub mod list;
pub mod list_project;
pub mod publish;
pub mod retire;
pub mod set_latest;
pub mod show;
pub mod versions;

use ds_cli_contract::{Failure, Inputs, spec::Refusal};
use serde_json::{Value, json};
pub const NATIVE_REFUSED: Refusal = Refusal {
    code: "design_attachment_refused",
    when: "The native attachment owner rejects identity, version pin, storage grant or pointer transaction",
    remedy: "Read the nested cause. Use explicit --project and correct LV vN/MV content revision; review a conflict without silently replacing its fence.",
};
pub fn object(inputs: &Inputs) -> Result<ds_client_core::design_attachments::Object, Failure> {
    Ok(ds_client_core::design_attachments::Object {
        kind: inputs.require("kind")?.into(),
        id: inputs.require("object")?.into(),
        version: inputs.value("version").map(str::to_owned),
        version_digest: inputs.value("version-digest").map(str::to_owned),
    })
}
pub fn ask(
    inputs: &Inputs,
    command: ds_client_core::design_attachments::Command,
) -> Result<Value, Failure> {
    ds_cli_auth::design_attachments_for_project(
        inputs.require("lane")?,
        inputs.require("project")?,
        &command,
    )
    .map_err(refused)
}
/// Every owner refusal under the one declared code, its own cause nested.
pub fn refused(error: Failure) -> Failure {
    Failure::failed("design_attachment_refused", error.to_string())
        .detail(json!({"cause":error.code(),"detail":error.detail_value()}))
        .remedy(error.remedy_text().unwrap_or("Use exact object version pins; LV=vN, MV=source content revision. Review conflict state deliberately before retrying."))
}

#[cfg(test)]
mod tests {
    /// An action the kernel's policy offers on a file or a revision row must
    /// have a registered verb behind it; `make_latest` had none until
    /// `set-latest`.
    #[test]
    fn every_policy_action_names_a_registered_attachment_command() {
        let registered: Vec<&str> = crate::DOMAIN.commands.iter().map(|c| c.id).collect();
        for (kind, command) in ds_command_kernel::design_attachments::ACTION_COMMANDS {
            assert!(
                registered.contains(&command),
                "the attachment policy offers `{kind}` through `{command}`, which is not registered"
            );
        }
    }
}
