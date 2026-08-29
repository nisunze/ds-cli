//! Paired, map-independent global DS Grid catalog governance.
use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, ArgKind, Authority, Availability, Chapter, Command, Effect, Example, Execution, Refusal,
};
use ds_cli_contract::{Context, Inputs};
use ds_cli_desktop::ops::{BridgeOp, invoke, paired};
use serde_json::{Value, json};

const READ_ACTION: Arg = Arg {
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
    ],
    summary: "Read-only exact catalog discovery action.",
};
const WRITE_ACTION: Arg = Arg {
    name: "action",
    kind: ArgKind::Value,
    value: "<action>",
    required: true,
    default: None,
    choices: &[
        "upload",
        "library-publish",
        "example-publish",
        "library-lifecycle",
        "example-lifecycle",
    ],
    summary: "Confirmation-required governed catalog write action.",
};
const PAYLOAD: Arg = Arg::value(
    "payload",
    "<json>",
    "Typed action body JSON (library, example, lifecycle, or fork fields).",
);
const PATH: Arg = Arg::value("path", "<path>", "Local artifact path for upload only.");
const PURPOSE: Arg = Arg::value(
    "purpose",
    "<purpose>",
    "Upload purpose: library_manifest, library_validation_report, library_asset, example_model, example_project, or example_preview.",
);
const VISIBILITY: Arg = Arg::value(
    "visibility",
    "<visibility>",
    "Upload visibility: public, organization, or private.",
);
const DESCRIPTOR: Arg = Arg::value("desktop-descriptor", "<path>", "Paired Desktop descriptor.");
const PREPARED: Arg = Arg::value(
    "prepared",
    "<directory>",
    "Directory containing the typed library.json or example.json preparation document and its named local files.",
);
const LIBRARY_ID: Arg = Arg::value("library-id", "<library-id>", "Global library id.");
const EXAMPLE_ID: Arg = Arg::value("example-id", "<example-id>", "Global example id.");
const EXPECTED_HEAD_RELEASE: Arg = Arg::value(
    "expected-head-release",
    "<release-id>",
    "Exact current library head release id; refuses if it moved.",
);
const EXPECTED_HEAD_REVISION: Arg = Arg::value(
    "expected-head-revision",
    "<revision-id>",
    "Exact current example head revision id; refuses if it moved.",
);
const EXPECTED_LIFECYCLE: Arg = Arg::value(
    "expected-lifecycle",
    "<state>",
    "Current lifecycle fence: active, archived, or deprecated.",
);
const LIFECYCLE: Arg = Arg::value(
    "lifecycle",
    "<state>",
    "Target lifecycle: active, archived, or deprecated.",
);
pub static READ_COMMAND: Command = Command {
    id: "library.global.read",
    path: &["library", "global", "read"],
    contract: 1,
    summary: "List or inspect exact global catalog libraries and examples.",
    purpose: "Map-independent signed-in catalog discovery only.",
    chapter: Chapter::PlsCadd,
    effect: Effect::ReadOnly,
    authority: Authority::DesktopUser,
    execution: Execution::Sync,
    args: &[READ_ACTION, PAYLOAD, DESCRIPTOR],
    output: "Bounded exact library/example heads or immutable release/revision histories.",
    examples: &[Example {
        command: "ds library global read --action library-releases --payload '{\"library_id\":\"rw-pls-cadd-structures\"}' --output json",
        note: "List the immutable releases of one governed global library without opening a map.",
        runnable: false,
    }],
    refusals: &[Refusal {
        code: "not_paired",
        when: "the Desktop session is unavailable",
        remedy: "pair ds with the signed-in Desktop application",
    }],
    reference: Some("docs/reference/library.md"),
    availability: || Availability::Available,
};
pub static WRITE_COMMAND: Command = Command {
    id: "library.global.write",
    path: &["library", "global", "write"],
    contract: 1,
    summary: "Legacy payload route for governed global catalog writes.",
    purpose: "Confirmation-required publisher action. Upload session URIs are never emitted.",
    chapter: Chapter::PlsCadd,
    effect: Effect::GlobalWrite,
    authority: Authority::DesktopUser,
    execution: Execution::Sync,
    args: &[WRITE_ACTION, PAYLOAD, PATH, PURPOSE, VISIBILITY, DESCRIPTOR],
    output: "Artifact pin, immutable published record, or fenced lifecycle receipt.",
    examples: &[Example {
        command: "ds library global write --action library-lifecycle --payload '{\"library_id\":\"rw-pls-cadd-structures\",\"expected_head_release_id\":\"2026.08\",\"lifecycle\":\"archived\"}' --yes --output json",
        note: "Archive the current library head with an optimistic head fence; immutable releases remain readable.",
        runnable: false,
    }],
    refusals: &[Refusal {
        code: "not_paired",
        when: "the Desktop session is unavailable",
        remedy: "pair ds with the signed-in Desktop application",
    }],
    reference: Some("docs/reference/library.md"),
    availability: || Availability::Available,
};
pub static FORK_COMMAND: Command = Command {
    id: "library.global.fork-example",
    path: &["library", "global", "fork-example"],
    contract: 1,
    summary: "Fork one exact global example revision into a project model.",
    purpose: "Project-authorized, map-independent exact fork; no source reupload/copy.",
    chapter: Chapter::PlsCadd,
    effect: Effect::GlobalWrite,
    authority: Authority::Project,
    execution: Execution::Sync,
    args: &[PAYLOAD, DESCRIPTOR],
    output: "The new immutable project model version with server-derived global provenance.",
    examples: &[Example {
        command: "ds library global fork-example --payload '{\"project_id\":\"my-project\",\"fork\":{\"example_id\":\"karongi-mv\",\"example_revision_id\":\"2026.08\",\"expected_head_revision_id\":\"2026.08\",\"model_id\":\"karongi-copy\",\"revision_id\":\"v1\",\"display_name\":\"Karongi governed copy\",\"model_kind\":\"mv_line\",\"model_schema_version\":\"1\",\"engine_version\":\"pls-cadd-pinned\",\"reason\":\"Start from the proven global example\"}}' --yes --output json",
        note: "Fork one exact active global example revision into an authorized project without copying or re-uploading its source object.",
        runnable: false,
    }],
    refusals: &[Refusal {
        code: "not_paired",
        when: "the Desktop session is unavailable",
        remedy: "pair ds with the signed-in Desktop application",
    }],
    reference: Some("docs/reference/library.md"),
    availability: || Availability::Available,
};
pub static UPLOAD_COMMAND: Command = Command {
    id: "library.global.upload",
    path: &["library", "global", "upload"],
    contract: 1,
    summary: "Upload one typed global catalogue artifact from a local file.",
    purpose: "Publisher-only, map-independent content-addressed upload. It returns an immutable artifact pin and never exposes the resumable session URI, publishes a release, or evaluates a native model.",
    chapter: Chapter::PlsCadd,
    effect: Effect::GlobalWrite,
    authority: Authority::DesktopUser,
    execution: Execution::Sync,
    args: &[PATH, PURPOSE, VISIBILITY, DESCRIPTOR],
    output: "One canonical digest, object and byte-length artifact pin.",
    examples: &[Example {
        command: "ds library global upload --path ./pls-cadd/criteria.cri --purpose library_asset --visibility organization --yes --output json",
        note: "Upload one opaque library asset; it is not a solver approval.",
        runnable: false,
    }],
    refusals: &[Refusal {
        code: "not_paired",
        when: "the Desktop session is unavailable",
        remedy: "pair ds with the signed-in Desktop application",
    }],
    reference: Some("docs/reference/library.md"),
    availability: || Availability::Available,
};
pub static PUBLISH_LIBRARY_COMMAND: Command = Command {
    id: "library.global.publish-library",
    path: &["library", "global", "publish-library"],
    contract: 1,
    summary: "Create or advance a governed global library from a prepared directory.",
    purpose: "Publisher-only, map-independent publication. The paired desktop reads library.json and its explicitly named files, uploads manifest, validation evidence and typed assets under closed purposes, then publishes one immutable release. It never overwrites a release or approves a native solver result.",
    chapter: Chapter::PlsCadd,
    effect: Effect::GlobalWrite,
    authority: Authority::DesktopUser,
    execution: Execution::Sync,
    args: &[PREPARED, DESCRIPTOR],
    output: "The mutable library head pointing to one newly published or idempotently retried immutable release.",
    examples: &[Example {
        command: "ds library global publish-library --prepared ./karongi-library --yes --output json",
        note: "Publish the typed library.json preparation directory without a raw API JSON argument.",
        runnable: false,
    }],
    refusals: &[Refusal {
        code: "not_paired",
        when: "the Desktop session is unavailable",
        remedy: "pair ds with the signed-in Desktop application",
    }],
    reference: Some("docs/reference/library.md"),
    availability: || Availability::Available,
};
pub static PUBLISH_EXAMPLE_COMMAND: Command = Command {
    id: "library.global.publish-example",
    path: &["library", "global", "publish-example"],
    contract: 1,
    summary: "Create or advance a governed global example from a prepared directory.",
    purpose: "Publisher-only, map-independent publication. The paired desktop reads example.json and its named model, project-plane and preview files, uploads them under closed purposes, and pins one exact library release. It does not run or approve a solver.",
    chapter: Chapter::PlsCadd,
    effect: Effect::GlobalWrite,
    authority: Authority::DesktopUser,
    execution: Execution::Sync,
    args: &[PREPARED, DESCRIPTOR],
    output: "The mutable example head pointing to one newly published or idempotently retried immutable revision.",
    examples: &[Example {
        command: "ds library global publish-example --prepared ./karongi-example --yes --output json",
        note: "Publish the typed example.json preparation directory without a raw API JSON argument.",
        runnable: false,
    }],
    refusals: &[Refusal {
        code: "not_paired",
        when: "the Desktop session is unavailable",
        remedy: "pair ds with the signed-in Desktop application",
    }],
    reference: Some("docs/reference/library.md"),
    availability: || Availability::Available,
};
pub static LIBRARY_LIFECYCLE_COMMAND: Command = Command {
    id: "library.global.library-lifecycle",
    path: &["library", "global", "library-lifecycle"],
    contract: 1,
    summary: "Archive, deprecate, or restore a global library head with fences.",
    purpose: "Publisher-only mutable-head management. It requires both exact head and lifecycle fences; immutable releases are never changed or deleted.",
    chapter: Chapter::PlsCadd,
    effect: Effect::GlobalWrite,
    authority: Authority::DesktopUser,
    execution: Execution::Sync,
    args: &[
        LIBRARY_ID,
        EXPECTED_HEAD_RELEASE,
        EXPECTED_LIFECYCLE,
        LIFECYCLE,
        DESCRIPTOR,
    ],
    output: "The fenced library head lifecycle receipt.",
    examples: &[Example {
        command: "ds library global library-lifecycle --library-id karongi --expected-head-release r1 --expected-lifecycle active --lifecycle archived --yes --output json",
        note: "Archive a library head while retaining every immutable release.",
        runnable: false,
    }],
    refusals: &[Refusal {
        code: "not_paired",
        when: "the Desktop session is unavailable",
        remedy: "pair ds with the signed-in Desktop application",
    }],
    reference: Some("docs/reference/library.md"),
    availability: || Availability::Available,
};
pub static EXAMPLE_LIFECYCLE_COMMAND: Command = Command {
    id: "library.global.example-lifecycle",
    path: &["library", "global", "example-lifecycle"],
    contract: 1,
    summary: "Archive, deprecate, or restore a global example head with fences.",
    purpose: "Publisher-only mutable-head management. It requires both exact head and lifecycle fences; immutable example revisions are never changed or deleted.",
    chapter: Chapter::PlsCadd,
    effect: Effect::GlobalWrite,
    authority: Authority::DesktopUser,
    execution: Execution::Sync,
    args: &[
        EXAMPLE_ID,
        EXPECTED_HEAD_REVISION,
        EXPECTED_LIFECYCLE,
        LIFECYCLE,
        DESCRIPTOR,
    ],
    output: "The fenced example head lifecycle receipt.",
    examples: &[Example {
        command: "ds library global example-lifecycle --example-id karongi --expected-head-revision r1 --expected-lifecycle archived --lifecycle active --yes --output json",
        note: "Restore a governed example before it can seed a new project.",
        runnable: false,
    }],
    refusals: &[Refusal {
        code: "not_paired",
        when: "the Desktop session is unavailable",
        remedy: "pair ds with the signed-in Desktop application",
    }],
    reference: Some("docs/reference/library.md"),
    availability: || Availability::Available,
};
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
const LPP: BridgeOp = BridgeOp {
    operation: "catalog.library.publish-prepared",
    arguments: &["prepared"],
};
const EPP: BridgeOp = BridgeOp {
    operation: "catalog.example.publish-prepared",
    arguments: &["prepared"],
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
    &LIST, &READ, &RELEASES, &EXAMPLES, &REVISIONS, &UPLOAD, &LP, &EP, &LPP, &EPP, &LL, &EL, &FORK,
];
fn run_allowed(inputs: &Inputs, allowed: &[&str]) -> Result<Value, Failure> {
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
    if !allowed.contains(&action) {
        return Err(Failure::invalid(
            "catalog_action_not_allowed",
            format!("{action} is not allowed by this command's effect and authority"),
        ));
    }
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
            ));
        }
    };
    let descriptor = paired(inputs.value("desktop-descriptor"))?;
    invoke(&descriptor, op, args, std::time::Duration::from_secs(120))
}
pub fn run_read(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
    run_allowed(
        inputs,
        &[
            "library-list",
            "library-read",
            "library-releases",
            "example-list",
            "example-revisions",
        ],
    )
}
pub fn run_write(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
    run_allowed(
        inputs,
        &[
            "upload",
            "library-publish",
            "example-publish",
            "library-lifecycle",
            "example-lifecycle",
        ],
    )
}
pub fn run_fork(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
    let payload: Value = inputs
        .value("payload")
        .ok_or_else(|| Failure::invalid("catalog_payload_invalid", "--payload is required"))
        .and_then(|value| {
            serde_json::from_str(value)
                .map_err(|_| Failure::invalid("catalog_payload_invalid", "--payload must be JSON"))
        })?;
    let descriptor = paired(inputs.value("desktop-descriptor"))?;
    invoke(
        &descriptor,
        &FORK,
        payload,
        std::time::Duration::from_secs(120),
    )
}
fn run_structured(inputs: &Inputs, op: &BridgeOp, args: Value) -> Result<Value, Failure> {
    let descriptor = paired(inputs.value("desktop-descriptor"))?;
    invoke(&descriptor, op, args, std::time::Duration::from_secs(120))
}
pub fn run_upload(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
    run_structured(
        inputs,
        &UPLOAD,
        json!({"path": inputs.require("path")?, "purpose": inputs.require("purpose")?, "visibility": inputs.require("visibility")?}),
    )
}
pub fn run_publish_library(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
    run_structured(
        inputs,
        &LPP,
        json!({"prepared": inputs.require("prepared")?}),
    )
}
pub fn run_publish_example(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
    run_structured(
        inputs,
        &EPP,
        json!({"prepared": inputs.require("prepared")?}),
    )
}
pub fn run_library_lifecycle(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
    run_structured(
        inputs,
        &LL,
        json!({"library_lifecycle": {"library_id": inputs.require("library-id")?, "expected_head_release_id": inputs.require("expected-head-release")?, "expected_lifecycle": inputs.require("expected-lifecycle")?, "lifecycle": inputs.require("lifecycle")?}}),
    )
}
pub fn run_example_lifecycle(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
    run_structured(
        inputs,
        &EL,
        json!({"example_lifecycle": {"example_id": inputs.require("example-id")?, "expected_head_revision_id": inputs.require("expected-head-revision")?, "expected_lifecycle": inputs.require("expected-lifecycle")?, "lifecycle": inputs.require("lifecycle")?}}),
    )
}
pub fn render(data: &Value) -> String {
    serde_json::to_string_pretty(data).unwrap_or_else(|_| "catalog result".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_and_publisher_actions_are_disjoint() {
        let read = READ_COMMAND.args[0].choices;
        let write = WRITE_COMMAND.args[0].choices;

        assert!(read.iter().all(|action| !write.contains(action)));
        assert!(read.contains(&"library-list"));
        assert!(!read.contains(&"library-publish"));
        assert!(write.contains(&"library-publish"));
        assert!(!write.contains(&"library-list"));
    }

    #[test]
    fn exact_project_fork_has_no_action_multiplexer() {
        assert_eq!(FORK_COMMAND.id, "library.global.fork-example");
        assert!(FORK_COMMAND.args.iter().all(|arg| arg.name != "action"));
        assert_eq!(FORK.operation, "catalog.fork-example");
    }

    #[test]
    fn prepared_publication_and_lifecycle_are_not_raw_payload_commands() {
        for command in [
            &UPLOAD_COMMAND,
            &PUBLISH_LIBRARY_COMMAND,
            &PUBLISH_EXAMPLE_COMMAND,
            &LIBRARY_LIFECYCLE_COMMAND,
            &EXAMPLE_LIFECYCLE_COMMAND,
        ] {
            assert!(command.args.iter().all(|arg| arg.name != "payload"));
        }
        assert_eq!(LPP.operation, "catalog.library.publish-prepared");
        assert_eq!(EPP.operation, "catalog.example.publish-prepared");
        assert!(PURPOSE.summary.contains("library_asset"));
        assert!(PURPOSE.summary.contains("example_project"));
    }
}
