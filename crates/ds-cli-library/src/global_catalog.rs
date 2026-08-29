//! Paired, map-independent global DS Grid catalog governance.
use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, ArgKind, Authority, Availability, Chapter, Command, Effect, Execution, Refusal,
};
use ds_cli_contract::{Context, Inputs};
use ds_cli_desktop::ops::{invoke, paired, BridgeOp};
use serde_json::{json, Value};

const ACTION: Arg = Arg {
    name: "action",
    kind: ArgKind::Value,
    value: "<action>",
    required: true,
    default: None,
    choices: &[
        "library-list",
        "library-read",
        "library-releases",
        "example-list",
        "example-revisions",
        "upload",
        "library-publish",
        "example-publish",
        "library-lifecycle",
        "example-lifecycle",
        "fork-example",
    ],
    summary: "One named catalog operation; list/read are map-independent.",
};
const PAYLOAD: Arg = Arg::value(
    "payload",
    "<json>",
    "Typed action body JSON (library, example, lifecycle, or fork fields).",
);
const PATH: Arg = Arg::value("path", "<path>", "Local artifact path for upload only.");
const PURPOSE: Arg = Arg::value("purpose", "<purpose>", "Upload purpose: library_manifest, library_validation_report, example_model, or example_preview.");
const VISIBILITY: Arg = Arg::value(
    "visibility",
    "<visibility>",
    "Upload visibility: public, organization, or private.",
);
const DESCRIPTOR: Arg = Arg::value("desktop-descriptor", "<path>", "Paired Desktop descriptor.");
pub static READ_COMMAND: Command = Command { id:"library.global.read", path:&["library","global","read"], contract:1, summary:"List or inspect exact global catalog libraries and examples.", purpose:"Map-independent signed-in catalog discovery only. The action is one of library-list, library-read, library-releases, example-list, or example-revisions; no governed state changes.", chapter:Chapter::PlsCadd, effect:Effect::ReadOnly, authority:Authority::DesktopUser, execution:Execution::Sync, args:&[ACTION,PAYLOAD,DESCRIPTOR], output:"Bounded exact library/example heads or immutable release/revision histories.", examples:&[], refusals:&[Refusal{code:"not_paired",when:"the Desktop session is unavailable",remedy:"pair ds with the signed-in Desktop application"}], reference:Some("docs/reference/library.md"), availability:|| Availability::Available };
pub static WRITE_COMMAND: Command = Command { id:"library.global.write", path:&["library","global","write"], contract:1, summary:"Upload or govern immutable global catalog releases and example revisions.", purpose:"Confirmation-required publisher action. It uploads bytes through a server-minted resumable session without emitting that session URI, publishes immutable records, or advances a lifecycle head behind an expected-head fence.", chapter:Chapter::PlsCadd, effect:Effect::GlobalWrite, authority:Authority::DesktopUser, execution:Execution::Sync, args:&[ACTION,PAYLOAD,PATH,PURPOSE,VISIBILITY,DESCRIPTOR], output:"Artifact pin, immutable published record, or fenced lifecycle receipt.", examples:&[], refusals:&[Refusal{code:"not_paired",when:"the Desktop session is unavailable",remedy:"pair ds with the signed-in Desktop application"}], reference:Some("docs/reference/library.md"), availability:|| Availability::Available };
pub static FORK_COMMAND: Command = Command { id:"library.global.fork-example", path:&["library","global","fork-example"], contract:1, summary:"Fork one exact global example revision into a project model.", purpose:"Project-authorized, map-independent exact fork. Pass --action fork-example and the server pins global provenance and reuses the verified source object; it does not upload or copy model bytes.", chapter:Chapter::PlsCadd, effect:Effect::GlobalWrite, authority:Authority::Project, execution:Execution::Sync, args:&[ACTION,PAYLOAD,DESCRIPTOR], output:"The new immutable project model version with server-derived global provenance.", examples:&[], refusals:&[Refusal{code:"not_paired",when:"the Desktop session is unavailable",remedy:"pair ds with the signed-in Desktop application"}], reference:Some("docs/reference/library.md"), availability:|| Availability::Available };
const LIST: BridgeOp = BridgeOp {
    operation: "catalog.library.list",
    arguments: &[],
};
const READ: BridgeOp = BridgeOp {
    operation: "catalog.library.read",
    arguments: &["library_id", "release_id"],
};
const RELEASES: BridgeOp = BridgeOp {
    operation: "catalog.library.releases",
    arguments: &["library_id"],
};
const EXAMPLES: BridgeOp = BridgeOp {
    operation: "catalog.example.list",
    arguments: &[],
};
const REVISIONS: BridgeOp = BridgeOp {
    operation: "catalog.example.revisions",
    arguments: &["example_id"],
};
const UPLOAD: BridgeOp = BridgeOp {
    operation: "catalog.artifact.upload",
    arguments: &["path", "purpose", "visibility"],
};
const LP: BridgeOp = BridgeOp {
    operation: "catalog.library.publish",
    arguments: &["library"],
};
const EP: BridgeOp = BridgeOp {
    operation: "catalog.example.publish",
    arguments: &["example"],
};
const LL: BridgeOp = BridgeOp {
    operation: "catalog.library.lifecycle",
    arguments: &["library_lifecycle"],
};
const EL: BridgeOp = BridgeOp {
    operation: "catalog.example.lifecycle",
    arguments: &["example_lifecycle"],
};
const FORK: BridgeOp = BridgeOp {
    operation: "catalog.fork-example",
    arguments: &["project_id", "fork"],
};
pub const BRIDGE_OPS: &[&BridgeOp] = &[
    &LIST, &READ, &RELEASES, &EXAMPLES, &REVISIONS, &UPLOAD, &LP, &EP, &LL, &EL, &FORK,
];
pub fn run(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
    let payload: Value = inputs
        .value("payload")
        .map(|v| {
            serde_json::from_str(v)
                .map_err(|_| Failure::invalid("catalog_payload_invalid", "--payload must be JSON"))
        })
        .transpose()?
        .unwrap_or_else(|| json!({}));
    let object = payload.as_object().cloned().ok_or_else(|| {
        Failure::invalid("catalog_payload_invalid", "--payload must be an object")
    })?;
    let action = inputs.require("action")?;
    let (op, args) = match action {
        "library-list" => (&LIST, json!({})),
        "library-read" => (&READ, payload),
        "library-releases" => (&RELEASES, payload),
        "example-list" => (&EXAMPLES, json!({})),
        "example-revisions" => (&REVISIONS, payload),
        "upload" => (
            &UPLOAD,
            json!({"path":inputs.require("path")?,"purpose":inputs.require("purpose")?,"visibility":inputs.require("visibility")?}),
        ),
        "library-publish" => (&LP, json!({"library":object})),
        "example-publish" => (&EP, json!({"example":object})),
        "library-lifecycle" => (&LL, json!({"library_lifecycle":object})),
        "example-lifecycle" => (&EL, json!({"example_lifecycle":object})),
        "fork-example" => (&FORK, payload),
        _ => {
            return Err(Failure::invalid(
                "catalog_action_invalid",
                "unknown catalog action",
            ))
        }
    };
    let descriptor = paired(inputs.value("desktop-descriptor"))?;
    invoke(&descriptor, op, args, std::time::Duration::from_secs(120))
}
pub fn render(data: &Value) -> String {
    serde_json::to_string_pretty(data).unwrap_or_else(|_| "catalog result".into())
}
