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
    value: "<absolute-path.dsgrid|bak>",
    required: false,
    default: None,
    choices: &[],
    summary: "Exact .dsgrid or, with --replace-content, PLS-CADD .bak for headless publication.",
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
    summary: "Why this revision exists. Stored with the revision.",
};

/// The governance a revision can carry, stored by the catalog with it.
const GOVERNANCE_ARGS: [Arg; 10] = [
    Arg::switch(
        "bump-version",
        "Start the next version with this revision; without it the save joins the current version.",
    ),
    Arg::value(
        "milestone",
        "<text>",
        "Milestone label, e.g. a submission (at most 300 characters).",
    ),
    Arg::value(
        "description",
        "<text>",
        "Revision description (at most 2000 characters).",
    ),
    Arg::value(
        "design-stage",
        "<id>",
        "Design stage id recorded on the revision.",
    ),
    Arg::value(
        "detail-level",
        "<id>",
        "Detail level id recorded on the revision.",
    ),
    Arg::value(
        "approval",
        "<status>",
        "Review state recorded on the revision.",
    )
    .choices(&["draft", "submitted", "approved", "rejected"]),
    Arg::value(
        "approval-level",
        "<id>",
        "Approval level id; required for approved or rejected.",
    ),
    Arg::value(
        "approval-reason",
        "<text>",
        "Decision reason; required for approved or rejected.",
    ),
    Arg::repeated(
        "operation-summary",
        "<text>",
        "One line of what this revision did; repeat up to 100.",
    ),
    Arg::repeated(
        "composition-source",
        "<model:revision>",
        "Ordered project revision this one composes; repeat 2..100.",
    ),
];
const ATTACH_ARG: Arg = Arg::repeated(
    "attach",
    "<path[:purpose]>",
    "File to attach to the NEW revision after it commits, e.g. the delivered .bak; repeatable.",
);

pub const AMBIGUOUS_SOURCE: Refusal = Refusal {
    code: "ambiguous_publish_source",
    when: "both --model and --path name a source",
    remedy: "name one source, or neither to publish the model you are working in",
};
pub const RENAME_UNSUPPORTED: Refusal = Refusal {
    code: "project_model_rename_unsupported",
    when: "--name is given with --project-model",
    remedy: "drop --name; rename an existing project model with ds dsgrid project update --name",
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
pub const GOVERNANCE_INVALID: Refusal = Refusal {
    code: "publish_governance_invalid",
    when: "a milestone, stage, detail, approval, operation summary, composition source or bump is outside the catalog's rules",
    remedy: "read the named field in the message; ids match ^[a-z0-9][a-z0-9_-]{1,127}$, a decision needs --approval-level and --approval-reason",
};
pub const ATTACHMENT_INVALID: Refusal = Refusal {
    code: "attachment_file_invalid",
    when: "an --attach file is missing, unreadable, empty or above 512 MiB",
    remedy: "name existing files; each is checked before anything is published",
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
    GOVERNANCE_INVALID,
    ATTACHMENT_INVALID,
    Refusal {
        code: "replace_content_target_required",
        when: "--replace-content lacks --path, --project, --project-model or --expected-head",
        remedy: "name the exact incoming file and reviewed existing project head",
    },
    Refusal {
        code: "backup_crs_required",
        when: "a .bak replacement lacks --crs",
        remedy: "declare its projected native CRS",
    },
    Refusal {
        code: "backup_selection_invalid",
        when: "--select-project does not resolve one PLS project",
        remedy: "inspect the backup and name one exact .don leaf",
    },
    Refusal {
        code: "backup_conversion_blocked",
        when: "the exchange plan has blockers or losses",
        remedy: "resolve the reported CRS, project selection or native source finding",
    },
    Refusal {
        code: "backup_conversion_failed",
        when: "the exchange did not produce exactly one valid .dsgrid",
        remedy: "read the per-source conversion finding",
    },
    Refusal {
        code: "replace_content_invalid",
        when: "the incoming or downloaded head package cannot be validated or imported",
        remedy: "inspect both packages and the selected project head",
    },
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
        remedy: "provide an existing .dsgrid or .bak path",
    },
    Refusal {
        code: "model_unreadable",
        when: "the source cannot be read",
        remedy: "check file permissions",
    },
    Refusal {
        code: "not_a_dsgrid_package",
        when: "the source is not a valid container",
        remedy: "provide a valid .dsgrid, or import a .bak with --replace-content and --crs",
    },
    Refusal {
        code: "manifest_unreadable",
        when: "the source manifest is incompatible",
        remedy: "use a matching Network release",
    },
];
const fn publication_refusals() -> [Refusal; LOCAL_REFUSALS.len() + crate::project::SHARED] {
    let mut result = [CONFIRMATION_REQUIRED; LOCAL_REFUSALS.len() + crate::project::SHARED];
    let mut i = 0;
    while i < LOCAL_REFUSALS.len() {
        result[i] = LOCAL_REFUSALS[i];
        i += 1;
    }
    crate::project::with_shared(result, LOCAL_REFUSALS.len())
}

const SOURCE_ARGS: [Arg; 14] = [
    MODEL_ARG,
    PATH_ARG,
    Arg::switch(
        "replace-content",
        "Import incoming .dsgrid or .bak content as the next revision of an existing project model.",
    ),
    Arg::value(
        "crs",
        "<code>",
        "Declared projected CRS required for a PLS-CADD .bak source.",
    ),
    Arg::value(
        "select-project",
        "<don-leaf>",
        "Exact .don project leaf when a .bak holds several projects.",
    ),
    Arg::switch(
        "swap-xy",
        "Apply the explicit X/Y correction while importing a PLS-CADD .bak.",
    ),
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
    ATTACH_ARG,
];
const ARG_COUNT: usize = SOURCE_ARGS.len() + GOVERNANCE_ARGS.len() + 1;
/// Source, then governance, then the paired descriptor: one declaration.
const fn publication_args() -> [Arg; ARG_COUNT] {
    let mut all = [DESCRIPTOR_ARG; ARG_COUNT];
    let mut i = 0;
    while i < SOURCE_ARGS.len() {
        all[i] = SOURCE_ARGS[i];
        i += 1;
    }
    let mut g = 0;
    while g < GOVERNANCE_ARGS.len() {
        all[i + g] = GOVERNANCE_ARGS[g];
        g += 1;
    }
    all
}
const ARGS: [Arg; ARG_COUNT] = publication_args();

pub static COMMAND: Command = Command {
    id: "dsgrid.publish-version",
    path: &["dsgrid", "publish-version"],
    contract: 3,
    summary: "Save a verified model revision into its project version.",
    purpose: "With --path and --project, validate and save exact model bytes as a new revision without Desktop. A save joins the model's current version; --bump-version deliberately starts the next one (for example a submission), and a superseded version never takes new saves. --milestone, --approval, --design-stage, --detail-level, --description, --operation-summary and --composition-source are stored on the revision; --attach adds files (the delivered .bak) to the new revision after it commits. A new model converted from PLS-CADD records the package's own workspace origin as typed migration evidence; --replace-content imports incoming .dsgrid or .bak content (a .bak needs explicit --crs) as the next revision of an existing model, its provenance recorded in the operation summary. Without --path, publishes the selected Desktop working copy. Publication never changes the active local model.",
    chapter: Chapter::GridModel,
    effect: Effect::GlobalWrite,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &ARGS,
    output: "Published project/model/revision, version, revision_ordinal_within_version, version_revision_count, version_started, milestone, approval, kind, parent head, exact digest and byte length, and attachments[] with any attachment that failed named (the revision stands). Replacement adds source/head/result attestations and any backup conversion receipt; active_model_changed=false.",
    examples: &[
        Example {
            command: "ds dsgrid publish-version --path /work/route.dsgrid --project <exact-id> --name \"Kamonyi MV\" --kind mv_line --yes",
            note: "Publish a new model (v1) without Desktop.",
            runnable: false,
        },
        Example {
            command: "ds dsgrid publish-version --path /work/route.dsgrid --project <p> --project-model <m> --expected-head <rev> --kind mv_line --reason \"Respotted span 14\" --yes",
            note: "Save a design iteration into the current version.",
            runnable: false,
        },
        Example {
            command: "ds dsgrid publish-version --path /work/route.dsgrid --project <p> --project-model <m> --expected-head <rev> --kind mv_line --bump-version --milestone \"Submission 2\" --approval submitted --attach /work/delivered.bak:native_workspace --yes",
            note: "Start the submitted version and attach the delivered backup.",
            runnable: false,
        },
        Example {
            command: "ds dsgrid publish-version --path /work/replacement.bak --replace-content --crs EPSG:32735 --project <project> --project-model <model> --expected-head <revision> --kind mv_line --yes",
            note: "Import a backup as the next revision of an existing model.",
            runnable: false,
        },
    ],
    refusals: &publication_refusals(),
    reference: Some("docs/reference/dsgrid.md"),
    // One command, two routes. The declared authority is the route this
    // executable owns: `--path` publishes through the native owner with an
    // explicit project and no application at all. Without `--path` the
    // selected Desktop working copy is published through the paired
    // application, and dispatch still arbitrates that fallback exactly as a
    // `project` command (registry.rs `scope_headless_identity`), so the
    // Desktop's project must match the caller's.
    search: &["save revision", "bump version", "submission", "milestone"],
    requires: Requires::Server,
    availability: || ds_cli_contract::spec::Availability::Available,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let mut arguments = Map::new();

    // One selector. Two would leave the application to decide which source the
    // operator meant, and it has no way to know.
    let model = inputs.value("model");
    let path = inputs.value("path");
    let replace_content = inputs.switch("replace-content");
    if replace_content
        && (path.is_none()
            || inputs.value("project").is_none()
            || inputs.value("project-model").is_none()
            || inputs.value("expected-head").is_none())
    {
        return Err(Failure::invalid(
            "replace_content_target_required",
            "--replace-content needs a file, explicit project, existing project model and reviewed expected head",
        ));
    }
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
    // Revision governance, an explicit bump and attachments are stored by the
    // native owner; the paired route's closed operation carries none of them.
    if path.is_none()
        && let Some(flag) = GOVERNANCE_ARGS
            .iter()
            .chain([&ATTACH_ARG])
            .find(|arg| match arg.kind {
                ArgKind::Switch => inputs.switch(arg.name),
                ArgKind::Repeated => !inputs.repeated(arg.name).is_empty(),
                _ => inputs.value(arg.name).is_some(),
            })
    {
        return Err(Failure::invalid(
            "publish_native_path_required",
            format!(
                "--{} is stored by native publication, which needs --path and --project",
                flag.name
            ),
        )
        .remedy("provide --path and --project for native publication"));
    }
    if let Some(path) = path {
        arguments.insert(
            "path".into(),
            json!(if replace_content {
                replace_source_path(path)?
            } else {
                crate::model::external_dsgrid_path(path, "path")?
            }),
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

fn replace_source_path(raw: &str) -> Result<String, Failure> {
    let path = std::path::Path::new(raw);
    if !path.is_absolute() {
        return Err(Failure::invalid(
            "absolute_path_required",
            "--path must be absolute",
        ));
    }
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("");
    if !extension.eq_ignore_ascii_case("dsgrid") && !extension.eq_ignore_ascii_case("bak") {
        return Err(Failure::invalid(
            "unsupported_model_source",
            "--replace-content accepts .dsgrid or .bak",
        ));
    }
    Ok(raw.to_owned())
}

pub fn render(data: &Value) -> String {
    let ordinal = data["revision_ordinal_within_version"]
        .as_u64()
        .map_or_else(|| "?".to_owned(), |n| n.to_string());
    let mut out = format!(
        "published {} v{} revision {} in {}{}\n",
        data["project_model"].as_str().unwrap_or("?"),
        data["version"].as_u64().unwrap_or(0),
        ordinal,
        data["project"].as_str().unwrap_or("?"),
        if data["version_started"] == true {
            " (new version)"
        } else {
            ""
        },
    );
    if let Some(milestone) = data["milestone"].as_str().filter(|m| !m.is_empty()) {
        out.push_str(&format!("  milestone  {milestone}\n"));
    }
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
    for attached in data["attachments"].as_array().into_iter().flatten() {
        match attached["error"].as_str() {
            None => out.push_str(&format!(
                "  attached   {} ({} bytes)\n",
                attached["file"].as_str().unwrap_or("?"),
                attached["bytes"]
            )),
            Some(code) => out.push_str(&format!(
                "  NOT attached {}: {code}\n",
                attached["file"].as_str().unwrap_or("?")
            )),
        }
    }
    out
}

#[cfg(test)]
mod replacement_tests {
    use super::*;
    use ds_cli_contract::{Format, Output, parse};

    fn inputs(flags: &[&str]) -> Inputs {
        parse(
            &COMMAND,
            &flags.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
        )
        .expect("declared command flags")
    }

    fn context() -> Context {
        Context {
            confirmed: true,
            output: Output {
                format: Format::Json,
                pretty: false,
                color: false,
            },
        }
    }

    fn pinned_flags(path: &str) -> Vec<&str> {
        vec![
            "--path",
            path,
            "--replace-content",
            "--project",
            "project",
            "--project-model",
            "model",
            "--expected-head",
            "revision",
            "--kind",
            "mv_line",
        ]
    }

    #[test]
    fn replacement_requires_an_existing_reviewed_target_before_reading_source() {
        let flags = inputs(&[
            "--path",
            "/missing.dsgrid",
            "--replace-content",
            "--project",
            "project",
            "--kind",
            "mv_line",
        ]);
        let error = run(&flags, &context()).unwrap_err();
        assert_eq!(error.code(), "replace_content_target_required");
    }

    #[test]
    fn backup_requires_declared_crs_before_reading_source() {
        let flags = inputs(&pinned_flags("/missing.bak"));
        let error = run(&flags, &context()).unwrap_err();
        assert_eq!(error.code(), "backup_crs_required");
    }

    #[test]
    fn invalid_backup_plan_refuses_before_auth_or_project_write() {
        let path =
            std::env::temp_dir().join(format!("dsgrid-invalid-backup-{}.bak", std::process::id()));
        std::fs::write(&path, b"invalid-backup").unwrap();
        let path_text = path.to_str().unwrap();
        let mut flags = pinned_flags(path_text);
        flags.extend(["--crs", "EPSG:32735"]);
        let error = run(&inputs(&flags), &context()).unwrap_err();
        std::fs::remove_file(path).unwrap();
        assert_eq!(error.code(), "backup_conversion_blocked");
    }
}
