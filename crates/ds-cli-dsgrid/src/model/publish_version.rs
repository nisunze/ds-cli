//! Immutable model publication. An explicit package and project use the
//! shared native owner; a selected working copy retains Desktop compatibility.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, ArgKind, Authority, Chapter, Command, Effect, Example, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Map, Value, json};

use crate::model::{
    ABSOLUTE_PATH_REQUIRED, AMBIGUOUS, DESCRIPTOR_ARG, LOCAL_MODEL_NOT_FOUND, MODEL_ARG,
    MODEL_KINDS, MODEL_TOO_LARGE, NOT_PAIRED, PAIRING_REJECTED, PROJECT_NOT_OPEN, PUBLISH_TIMEOUT,
    REFUSED, SIGNED_OUT, UNREACHABLE, UNREADABLE, UNSUPPORTED, UNSUPPORTED_MODEL_SOURCE,
};

const PATH_ARG: Arg = Arg {
    name: "path",
    kind: ArgKind::Value,
    value: "<absolute-path.dsgrid>",
    required: false,
    default: None,
    choices: &[],
    summary: "Exact .dsgrid file for headless publication; requires --project and --kind.",
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
    summary: "The project model's kind. Required for native --path publication and new models.",
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

const LOCAL_REFUSALS: &[Refusal] = &[
    NOT_PAIRED,
    PROJECT_NOT_OPEN,
    AMBIGUOUS,
    UNREACHABLE,
    PAIRING_REJECTED,
    REFUSED,
    UNSUPPORTED,
    UNREADABLE,
    SIGNED_OUT,
    AMBIGUOUS_SOURCE,
    ABSOLUTE_PATH_REQUIRED,
    UNSUPPORTED_MODEL_SOURCE,
    MODEL_TOO_LARGE,
    RENAME_UNSUPPORTED,
    NEW_PROJECT_MODEL_INCOMPLETE,
    PROJECT_MODEL_NOT_FOUND,
    HEAD_CONFLICT,
    Refusal {
        code: "publish_conflict",
        when: "native storage or revision authority reports a conflict",
        remedy: "review the current model head and exact stored revision before retrying",
    },
    LOCAL_MODEL_NOT_FOUND,
    CONFIRMATION_REQUIRED,
    Refusal {
        code: "model_invalid",
        when: "the captured model has validation findings",
        remedy: "run ds dsgrid validate and resolve its findings",
    },
    Refusal {
        code: "publish_expected_head_required",
        when: "native publication targets an existing model without its expected head",
        remedy: "pass --expected-head with the exact reviewed revision",
    },
    Refusal {
        code: "publish_native_path_required",
        when: "an explicit project is provided without a file",
        remedy: "provide --path for native publication",
    },
    Refusal {
        code: "model_not_found",
        when: "the source file does not exist",
        remedy: "provide an existing .dsgrid path",
    },
    Refusal {
        code: "model_unreadable",
        when: "the source cannot be read",
        remedy: "check file permissions",
    },
    Refusal {
        code: "not_a_dsgrid_package",
        when: "the source is not a valid container",
        remedy: "convert the source to .dsgrid first",
    },
    Refusal {
        code: "manifest_unreadable",
        when: "the source manifest is incompatible",
        remedy: "use a matching Network release",
    },
];
const fn publication_refusals()
-> [Refusal; LOCAL_REFUSALS.len() + ds_cli_auth::PROJECT_STATUS_COMMAND.refusals.len()] {
    let mut result = [CONFIRMATION_REQUIRED;
        LOCAL_REFUSALS.len() + ds_cli_auth::PROJECT_STATUS_COMMAND.refusals.len()];
    let mut i = 0;
    while i < LOCAL_REFUSALS.len() {
        result[i] = LOCAL_REFUSALS[i];
        i += 1;
    }
    let mut j = 0;
    while j < ds_cli_auth::PROJECT_STATUS_COMMAND.refusals.len() {
        result[i + j] = ds_cli_auth::PROJECT_STATUS_COMMAND.refusals[j];
        j += 1;
    }
    result
}

pub static COMMAND: Command = Command {
    id: "dsgrid.publish-version",
    path: &["dsgrid", "publish-version"],
    contract: 2,
    summary: "Publish a verified model revision from a file or Desktop.",
    purpose: "With --path and --project, the Rust owner validates and uploads exact bytes, commits against --expected-head, and verifies the saved revision without Desktop. --kind is required; an existing project model also requires --expected-head. The destination project is authorized by the gateway and never changes saved selection. Without --path, publishes the selected Desktop working copy through the existing paired flow. Publishing a revision never renames an existing model.",
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
        Arg {
            name: "project",
            kind: ArgKind::Value,
            value: "<project-id>",
            required: false,
            default: None,
            choices: &[],
            summary: "Explicit project for native --path publication; never changes active selection.",
        },
        Arg {
            name: "lane",
            kind: ArgKind::Value,
            value: "<stable|canary>",
            required: false,
            default: Some("stable"),
            choices: &["stable", "canary"],
            summary: "Native publication deployment lane.",
        },
        DESCRIPTOR_ARG,
    ],
    output: "Published project/model/revision, kind, expected and parent heads, digest and byte length. Native publication includes verified=true and upload_skipped after exact readback; the paired flow also reports its local working-copy binding. active_model_changed=false confirms publication did not switch a local model.",
    examples: &[Example {
        command: "ds dsgrid publish-version --path /work/route.dsgrid --project <exact-id> --name \"Kamonyi MV\" --kind mv_line --yes",
        note: "Publish a new model through the native server contract without an open map.",
        runnable: false,
    }],
    refusals: &publication_refusals(),
    reference: Some("docs/reference/dsgrid.md"),
    // The only command in the CLI that reaches the paired window without
    // declaring a paired availability: `--path` publishes through the native
    // owner with no application at all, and omitting it falls back to the
    // Desktop working copy. `requires` answers "can this run on a server",
    // and with `--path` it can — so `server` is the true answer, and the
    // paired route is the part still owed a headless form. Do not "fix" this
    // to `window`: that would report the native path as unavailable on the
    // machine it was built for.
    search: &[],
    requires: Requires::Server,
    availability: || ds_cli_contract::spec::Availability::Available,
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

    if let Some(path) = path {
        return super::publish_native::run(inputs, path);
    }
    if inputs.value("project").is_some() {
        return Err(Failure::invalid(
            "publish_native_path_required",
            "An explicit project requires --path to a captured .dsgrid package",
        )
        .remedy("provide --path for native publication"));
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
    if data["verified"] == true {
        out.push_str("  verified   exact published revision read back\n");
    }
    if data.get("active_model_changed").is_some() {
        out.push_str(&format!(
            "  active     {} (unchanged: {})\n",
            data["active_model"].as_str().unwrap_or("none"),
            !data["active_model_changed"].as_bool().unwrap_or(false)
        ));
    }
    if data["binding_recorded"] == false {
        out.push_str("  note       the version is committed; the local binding was not written\n");
    }
    out
}
