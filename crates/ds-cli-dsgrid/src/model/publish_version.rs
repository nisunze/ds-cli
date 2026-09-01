//! `ds dsgrid publish-version` — register ONE immutable revision of a local
//! model in the active project's DS Grid catalogue.
//!
//! This is the family's only project act, and it sits outside `ds dsgrid
//! model` for that reason: everything under that path is local and needs no
//! project, while this one resolves an exact catalogue revision and therefore
//! cannot run without the paired session's own selected project.
//!
//! Three things it deliberately does not do:
//!
//! * **It does not activate anything.** ds-brain's project head means
//!   non-deleted catalogue membership and many heads coexist; there is no
//!   durable exclusive "activate this revision for the project" authority in
//!   this stack. The receipt reports `active_model` and `active_model_changed`
//!   so "published" is never read as "now current".
//! * **It does not rename.** Against an existing project model, `--name` is
//!   refused here, before anything is captured: publishing a revision must not
//!   quietly become a metadata edit.
//! * **It does not name a project.** The destination is the project the paired
//!   application already has selected, re-checked after every await on its
//!   side and fenced by the invocation identity on this one.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, ArgKind, Authority, Chapter, Command, Effect, Example, Execution, Refusal,
};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Map, Value, json};

use crate::model::{
    ABSOLUTE_PATH_REQUIRED, AMBIGUOUS, AUTH_CONTEXT_MISMATCH, DESCRIPTOR_ARG,
    LOCAL_MODEL_NOT_FOUND, MODEL_ARG, MODEL_KINDS, MODEL_TOO_LARGE, NOT_PAIRED, PAIRING_REJECTED,
    PUBLISH_TIMEOUT, REFUSED, SIGNED_OUT, UNREACHABLE, UNREADABLE, UNSUPPORTED,
    UNSUPPORTED_MODEL_SOURCE,
};

const PATH_ARG: Arg = Arg {
    name: "path",
    kind: ArgKind::Value,
    value: "<absolute-path.dsgrid>",
    required: false,
    default: None,
    choices: &[],
    summary: "Publish an external .dsgrid instead; it is acquired locally first.",
};

const PROJECT_MODEL_ARG: Arg = Arg {
    name: "project-model",
    kind: ArgKind::Value,
    value: "<project-model-id>",
    required: false,
    default: None,
    choices: &[],
    summary: "Add a version to this existing project model. Omit to publish a new one.",
};

const KIND_ARG: Arg = Arg {
    name: "kind",
    kind: ArgKind::Value,
    value: "<kind>",
    required: false,
    default: None,
    choices: MODEL_KINDS,
    summary: "The project model's kind. Required when publishing a new project model.",
};

const NAME_ARG: Arg = Arg {
    name: "name",
    kind: ArgKind::Value,
    value: "<text>",
    required: false,
    default: None,
    choices: &[],
    summary: "Display name for a NEW project model. Refused against an existing one.",
};

const EXPECTED_HEAD_ARG: Arg = Arg {
    name: "expected-head",
    kind: ArgKind::Value,
    value: "<revision-id>",
    required: false,
    default: None,
    choices: &[],
    summary: "The head you confirmed against. A moved head is refused, never retried.",
};

const REASON_ARG: Arg = Arg {
    name: "reason",
    kind: ArgKind::Value,
    value: "<text>",
    required: false,
    default: None,
    choices: &[],
    summary: "Why this version exists. Stored with the revision.",
};

pub const AMBIGUOUS_SOURCE: Refusal = Refusal {
    code: "ambiguous_publish_source",
    when: "both --model and --path name a source",
    remedy: "name one source, or neither to publish the model you are working in",
};
pub const RENAME_UNSUPPORTED: Refusal = Refusal {
    code: "project_model_rename_unsupported",
    when: "--name is given with --project-model",
    remedy: "drop --name; rename an existing project model through its own metadata authority",
};
pub const NEW_PROJECT_MODEL_INCOMPLETE: Refusal = Refusal {
    code: "new_project_model_incomplete",
    when: "--project-model is omitted but --name or --kind is missing",
    remedy: "pass --name and --kind to publish a new project model, or name an existing one",
};
pub const PROJECT_MODEL_NOT_FOUND: Refusal = Refusal {
    code: "project_model_not_found",
    when: "--project-model names no model in the active project",
    remedy: "project ids are generated, never authored; omit --project-model to publish a new one",
};
pub const HEAD_CONFLICT: Refusal = Refusal {
    code: "publish_head_conflict",
    when: "the project model's head moved away from --expected-head",
    remedy: "re-read the head, review what changed, and publish again deliberately",
};
pub const CONFIRMATION_REQUIRED: Refusal = Refusal {
    code: "confirmation_required",
    when: "--yes was not given for a command that writes a project revision",
    remedy: "re-run with --yes once you intend to publish",
};

pub static COMMAND: Command = Command {
    id: "dsgrid.publish-version",
    path: &["dsgrid", "publish-version"],
    contract: 1,
    summary: "Publish one local model revision to the active project's catalogue.",
    purpose: "\
Registers ONE validated immutable revision of a local DS Grid model in the \
paired session's own selected project, through the same upload and \
create-version flow the application's Create version dialog uses. The source \
is one selector: --model, or --path for an external package acquired first, or \
neither for the model you are already working in. It does not activate \
anything — there is no durable exclusive project revision activation in this \
stack — and against an existing project model it refuses --name rather than \
becoming a rename. The project is never an argument.",
    chapter: Chapter::GridModel,
    effect: Effect::GlobalWrite,
    authority: Authority::Project,
    execution: Execution::Sync,
    args: &[
        MODEL_ARG,
        PATH_ARG,
        PROJECT_MODEL_ARG,
        KIND_ARG,
        NAME_ARG,
        EXPECTED_HEAD_ARG,
        REASON_ARG,
        DESCRIPTOR_ARG,
    ],
    output: "\
`status: published`, the `project`, `project_model`, `revision`, `version`, \
`kind`, `expected_head`, `parent_revision`, the uploaded `digest` and \
`byte_length`, the `local_model` and `local_revision` published from, \
`binding_recorded`, and `active_model` with `active_model_changed` — which \
publication never changes.",
    examples: &[Example {
        command: "ds dsgrid publish-version --name \"Kamonyi MV\" --kind mv_line --reason \"Spotted route\" --yes",
        note: "Without --yes dispatch refuses before the bridge is opened.",
        runnable: false,
    }],
    refusals: &[
        NOT_PAIRED,
        AMBIGUOUS,
        UNREACHABLE,
        PAIRING_REJECTED,
        REFUSED,
        UNSUPPORTED,
        UNREADABLE,
        SIGNED_OUT,
        AUTH_CONTEXT_MISMATCH,
        AMBIGUOUS_SOURCE,
        ABSOLUTE_PATH_REQUIRED,
        UNSUPPORTED_MODEL_SOURCE,
        MODEL_TOO_LARGE,
        RENAME_UNSUPPORTED,
        NEW_PROJECT_MODEL_INCOMPLETE,
        PROJECT_MODEL_NOT_FOUND,
        HEAD_CONFLICT,
        LOCAL_MODEL_NOT_FOUND,
        CONFIRMATION_REQUIRED,
    ],
    reference: Some("docs/reference/dsgrid.md"),
    availability: crate::model::paired_availability,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let mut arguments = Map::new();

    // One selector. Two would leave the application to decide which source the
    // operator meant, and it has no way to know.
    let model = inputs.value("model");
    let path = inputs.value("path");
    if model.is_some() && path.is_some() {
        return Err(Failure::invalid(
            "ambiguous_publish_source",
            "--model and --path both name a source to publish",
        )
        .remedy(AMBIGUOUS_SOURCE.remedy)
        .next("ds dsgrid publish-version --help"));
    }
    if let Some(model) = model {
        arguments.insert("model".into(), json!(model));
    }
    if let Some(path) = path {
        arguments.insert(
            "path".into(),
            json!(crate::model::external_dsgrid_path(path, "path")?),
        );
    }

    // A named project model must already exist — project resource ids are
    // generated, never authored — so `--name` against one is not a new model,
    // it is a rename request wearing a publication's clothes.
    let project_model = inputs.value("project-model");
    let name = inputs.value("name");
    let kind = inputs.value("kind");
    match project_model {
        Some(project_model) => {
            if name.is_some() {
                return Err(Failure::invalid(
                    "project_model_rename_unsupported",
                    "--name is only accepted when publishing a NEW project model",
                )
                .remedy(RENAME_UNSUPPORTED.remedy)
                .next("ds dsgrid publish-version --help"));
            }
            arguments.insert("project_model".into(), json!(project_model));
        }
        None => {
            if name.is_none() || kind.is_none() {
                return Err(Failure::invalid(
                    "new_project_model_incomplete",
                    "publishing a new project model needs both --name and --kind",
                )
                .remedy(NEW_PROJECT_MODEL_INCOMPLETE.remedy)
                .next("ds dsgrid publish-version --help")
                .detail(json!({ "name": name.is_some(), "kind": kind.is_some() })));
            }
            arguments.insert("name".into(), json!(name));
        }
    }
    for (flag, key) in [
        ("kind", "kind"),
        ("expected-head", "expected_head"),
        ("reason", "reason"),
    ] {
        if let Some(value) = inputs.value(flag) {
            arguments.insert(key.into(), json!(value));
        }
    }

    let descriptor = crate::model::paired(inputs.value("desktop-descriptor"))?;
    crate::model::invoke(
        &descriptor,
        &crate::model::MODEL_PUBLISH,
        Value::Object(arguments),
        PUBLISH_TIMEOUT,
    )
    .map_err(crate::model::classify)
}

pub fn render(data: &Value) -> String {
    let mut out = format!(
        "published {} v{} in {}\n",
        data["project_model"].as_str().unwrap_or("?"),
        data["version"].as_u64().unwrap_or(0),
        data["project"].as_str().unwrap_or("?"),
    );
    out.push_str(&format!(
        "  revision   {}\n  kind       {}\n  digest     {}\n  bytes      {}\n  from       {}\n",
        data["revision"].as_str().unwrap_or("—"),
        data["kind"].as_str().unwrap_or("—"),
        crate::model::truncate(data["digest"].as_str().unwrap_or("—"), 32),
        data["byte_length"].as_u64().unwrap_or(0),
        data["local_model"].as_str().unwrap_or("—"),
    ));
    // Publication is project state only. Saying so on every receipt is what
    // stops "published" being read as "now the model I am working in".
    out.push_str(&format!(
        "  active     {} (unchanged: {})\n",
        data["active_model"].as_str().unwrap_or("none"),
        !data["active_model_changed"].as_bool().unwrap_or(false),
    ));
    if !data["binding_recorded"].as_bool().unwrap_or(true) {
        out.push_str("  note       the version is committed; the local binding was not written\n");
    }
    out
}
