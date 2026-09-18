//! `ds design attachment` — immutable, versioned files on a design object.
//!
//! ```text
//!   list → publish | download | retire
//! ```
//!
//! An attachment is OPAQUE: a PLS-CADD `.bak`, a native workspace, a report, a
//! photo, a client deliverable. Nothing here parses it. Publishing a revision
//! never mutates earlier bytes — each revision owns its own object, its own
//! server-verified digest and its own storage generation — and a download comes
//! back signed by ds-brain and pinned to that generation, so `ds` never composes
//! a storage URL.

pub mod download;
pub mod list;
pub mod publish;
pub mod retire;

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
    })
}
pub fn ask(
    inputs: &Inputs,
    command: ds_client_core::design_attachments::Command,
) -> Result<Value, Failure> {
    ds_cli_auth::design_attachments_for_project(inputs.require("lane")?,inputs.require("project")?,&command).map_err(|error|Failure::failed("design_attachment_refused",error.to_string()).detail(json!({"cause":error.code(),"detail":error.detail_value()})).remedy(error.remedy_text().unwrap_or("Use exact object version pins; LV=vN, MV=source content revision. Review conflict state deliberately before retrying.")))
}
