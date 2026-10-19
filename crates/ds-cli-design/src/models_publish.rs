//! Save an exact package as a fenced revision of an existing project MV model.
use std::fs::{File, OpenOptions};
use std::io::Read;
use std::path::Path;

use ds_cli_contract::spec::{
    Arg, Authority, Chapter, Command, Effect, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Failure, Inputs};
use ds_command_kernel::grid_publication::{Intent, MAX_MODEL_BYTES, Plan};
use ds_design_workspace::grid_publication::{self, Request};
use serde_json::{Value, json};

const SOURCE: Refusal = Refusal {
    code: "model_publish_source_invalid",
    when: "--file is not an existing absolute ordinary .dsgrid file or cannot be read exactly",
    remedy: "Choose an absolute ordinary .dsgrid file; keep its bytes unchanged while publishing.",
};
const EXPECTED_HEAD: Refusal = Refusal {
    code: "publish_expected_head_required",
    when: "an existing model has no nonempty reviewed expected head",
    remedy: "Read design model list for the exact project and lane, then pass its head_revision_id as --expected-head.",
};
const RECEIPT: Refusal = Refusal {
    code: "model_publish_receipt_invalid",
    when: "the returned publication acknowledgement differs from the exact prepared source or lacks verified commit metadata",
    remedy: "Do not repeat the publish blindly; inspect the exact project model head and retained source digest before deciding what to do next.",
};
const PREPARATION: Refusal = Refusal {
    code: "model_publish_preparation_failed",
    when: "the native preparation owner returns an unrecognized refusal or changes the requested publication scope",
    remedy: "Read the named owner cause and use a compatible native Network component; no publication was attempted.",
};
const CONFIRMATION: Refusal = Refusal {
    code: "confirmation_required",
    when: "--yes was not supplied for the governed revision save",
    remedy: "Review the exact project, model, expected head and package, then pass --yes.",
};
const CONFLICT: Refusal = Refusal {
    code: "publish_conflict",
    when: "the expected head moved or this immutable revision conflicts with stored state",
    remedy: "Read the exact project model head and stored revision; review the conflict before another publish.",
};

const fn preparation_refusal(code: &'static str, when: &'static str) -> Refusal {
    Refusal {
        code,
        when,
        remedy: "Inspect the unchanged source and exact publication inputs against the named owner rule.",
    }
}
const fn engine_refusal(code: &'static str, when: &'static str) -> Refusal {
    Refusal {
        code,
        when,
        remedy: "Use a compatible native Network component and writable private disk staging.",
    }
}
const PREPARE_REFUSALS: &[Refusal] = &[
    preparation_refusal(
        "model_too_large",
        "the package is empty or exceeds the owner's 256 MiB bound",
    ),
    preparation_refusal("model_invalid", "Network refuses the source package"),
    preparation_refusal(
        "grid_publication_project_or_model_invalid",
        "the exact project or model identifier is invalid",
    ),
    preparation_refusal(
        "grid_publication_source_invalid",
        "the prepared source or expected-head identity is invalid",
    ),
    preparation_refusal(
        "grid_publication_revision_invalid",
        "the native source revision cannot name an immutable project revision",
    ),
    preparation_refusal(
        "grid_publication_metadata_invalid",
        "the reason or native source metadata violates the publication contract",
    ),
    preparation_refusal(
        "grid_publication_governance_invalid",
        "the native preparation returns invalid governance metadata",
    ),
    preparation_refusal(
        "grid_publication_approval_invalid",
        "the native preparation returns invalid approval metadata",
    ),
    preparation_refusal(
        "grid_publication_operation_summary_invalid",
        "the native preparation returns invalid operation summaries",
    ),
    preparation_refusal(
        "grid_publication_composition_invalid",
        "the native preparation returns invalid composition metadata",
    ),
    preparation_refusal(
        "grid_publication_migration_invalid",
        "the native preparation returns invalid migration metadata",
    ),
    preparation_refusal(
        "grid_publication_bump_without_head",
        "the native preparation returns an invalid version-bump intent",
    ),
    engine_refusal(
        "network_engine_missing",
        "the installed native Network component is absent",
    ),
    engine_refusal(
        "network_engine_incompatible",
        "the captured component lacks the publication protocol or capability",
    ),
    engine_refusal(
        "network_serialization_incompatible",
        "the captured component has an incompatible JSON byte profile",
    ),
    engine_refusal(
        "network_engine_path_invalid",
        "the explicitly selected development component path is not absolute",
    ),
    engine_refusal(
        "network_engine_invalid",
        "the captured component cannot be read within its bound",
    ),
    engine_refusal(
        "network_engine_unqualified",
        "the installed component lacks admitted source and dependency pins",
    ),
    engine_refusal(
        "network_engine_identity_invalid",
        "the component identity probe is incompatible or malformed",
    ),
    engine_refusal(
        "network_engine_changed",
        "the captured component changes during execution",
    ),
    engine_refusal(
        "network_engine_refused",
        "the component returns an unrecognized process refusal",
    ),
    engine_refusal(
        "network_scratch_invalid",
        "private ordinary disk staging is unavailable",
    ),
    engine_refusal(
        "network_request_io",
        "the exact publication input cannot be staged",
    ),
    engine_refusal(
        "network_input_changed",
        "the staged publication input changes during execution",
    ),
    engine_refusal(
        "network_receipt_bounded",
        "the component exceeds its control-receipt bound",
    ),
    engine_refusal(
        "network_receipt_invalid",
        "the component receipt differs from the frozen input or captured identity",
    ),
    engine_refusal(
        "network_refusal_invalid",
        "the component failure is not an admitted refusal",
    ),
    engine_refusal(
        "network_result_invalid",
        "the native publication facts are malformed or inconsistent",
    ),
    engine_refusal(
        "network_result_digest_mismatch",
        "the native facts artifact differs from its receipt digest",
    ),
    engine_refusal(
        "network_workers_invalid",
        "the native worker count is invalid",
    ),
    engine_refusal(
        "network_task_cancelled",
        "native publication preparation was cancelled",
    ),
    engine_refusal(
        "network_control_unavailable",
        "native process control is unavailable",
    ),
    engine_refusal(
        "network_control_invalid",
        "native process control is invalid",
    ),
    engine_refusal(
        "network_input_invalid",
        "the native publication request is invalid",
    ),
    engine_refusal(
        "network_output_invalid",
        "the native output directory is invalid",
    ),
    engine_refusal(
        "network_artifact_io",
        "the native facts artifact cannot be read",
    ),
    engine_refusal(
        "grid_task_bound_refused",
        "the opaque package exceeds the native task bound",
    ),
    engine_refusal(
        "callee_cancelled",
        "native publication preparation was cancelled",
    ),
    engine_refusal(
        "callee_timed_out",
        "native publication preparation exceeded its bounded deadline",
    ),
    engine_refusal(
        "callee_wait_failed",
        "the native process could not be awaited",
    ),
    engine_refusal(
        "callee_supervision_failed",
        "the native process could not be supervised",
    ),
    engine_refusal(
        "callee_stdin_failed",
        "the native process input could not be delivered",
    ),
];
const OWN: &[Refusal] = &[
    SOURCE,
    EXPECTED_HEAD,
    RECEIPT,
    PREPARATION,
    CONFIRMATION,
    CONFLICT,
];
const fn refusals()
-> [Refusal; OWN.len() + PREPARE_REFUSALS.len() + crate::models::LIST.refusals.len()] {
    let mut result =
        [SOURCE; OWN.len() + PREPARE_REFUSALS.len() + crate::models::LIST.refusals.len()];
    let mut at = 0;
    let mut i = 0;
    while i < OWN.len() {
        result[at] = OWN[i];
        at += 1;
        i += 1;
    }
    i = 0;
    while i < PREPARE_REFUSALS.len() {
        result[at] = PREPARE_REFUSALS[i];
        at += 1;
        i += 1;
    }
    i = 0;
    while i < crate::models::LIST.refusals.len() {
        result[at] = crate::models::LIST.refusals[i];
        at += 1;
        i += 1;
    }
    result
}

pub static COMMAND: Command = Command {
    id: "design.model.publish",
    path: &["design", "model", "publish"],
    contract: 1,
    summary: "Save a reviewed .dsgrid checkpoint to an MV model (needs --yes).",
    purpose: "Publish one exact local package as an immutable revision of an existing mv_line model in the explicitly named project and credential lane. Native Network preparation validates the opaque package and derives its source identity; the authenticated owner uploads, commits against --expected-head and verifies exact revision readback. This saves into the current version without renaming, creating a model or changing a local active model. Report upload is a separate workflow. A failed acknowledgement is never retried automatically.",
    chapter: Chapter::Design,
    effect: Effect::GlobalWrite,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[
        crate::PROJECT_ARG,
        Arg { required: true, default: None, ..crate::LANE_ARG },
        Arg::value("model", "<id>", "Exact existing project model_id from design model list.").required(),
        Arg::value("expected-head", "<revision-id>", "Exact reviewed head_revision_id; never refreshed or replaced automatically.").required(),
        Arg::value("file", "<absolute.dsgrid>", "Existing ordinary absolute .dsgrid file, nonempty and at most 256 MiB; never modified.").required(),
        Arg::value("reason", "<text>", "Reviewed checkpoint reason, using the publication owner's text rules.").required(),
    ],
    output: "Verified owner status, project/lane, project_model, immutable revision, current version, digest/byte_length, source local_revision, expected_head/parent_revision, upload_skipped and available stored ordinal. active_model_changed=false. No report publication or signed locator.",
    examples: &[],
    refusals: &refusals(),
    reference: Some("docs/reference/design.md"),
    search: &["mv checkpoint", "existing model"],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};

fn invalid(refusal: Refusal, message: impl std::fmt::Display) -> Failure {
    let message = message.to_string();
    let failure = match refusal.code {
        "model_publish_source_invalid"
        | "publish_expected_head_required"
        | "confirmation_required" => Failure::invalid(refusal.code, message),
        _ => Failure::failed(PREPARATION.code, message),
    };
    failure.remedy(refusal.remedy)
}

fn prepare_failure(message: String) -> Failure {
    let owner_code = message.split(':').next().unwrap_or("").to_owned();
    let code = owner_code.as_str();
    let failure = match code {
        "network_engine_missing" => Failure::unavailable(code, message),
        "network_engine_incompatible"
        | "network_serialization_incompatible"
        | "network_engine_path_invalid"
        | "network_engine_invalid"
        | "network_engine_unqualified"
        | "network_engine_identity_invalid"
        | "network_engine_changed"
        | "network_engine_refused"
        | "network_scratch_invalid"
        | "network_request_io"
        | "network_input_changed"
        | "network_receipt_bounded"
        | "network_receipt_invalid"
        | "network_refusal_invalid"
        | "network_result_invalid"
        | "network_result_digest_mismatch"
        | "network_workers_invalid"
        | "network_task_cancelled"
        | "network_control_unavailable"
        | "network_control_invalid"
        | "network_input_invalid"
        | "network_output_invalid"
        | "network_artifact_io"
        | "grid_task_bound_refused"
        | "callee_cancelled"
        | "callee_timed_out"
        | "callee_wait_failed"
        | "callee_supervision_failed"
        | "callee_stdin_failed" => Failure::failed(code, message),
        "publish_expected_head_required"
        | "model_too_large"
        | "model_invalid"
        | "composite_model_protected"
        | "grid_publication_project_or_model_invalid"
        | "grid_publication_source_invalid"
        | "grid_publication_revision_invalid"
        | "grid_publication_metadata_invalid"
        | "grid_publication_governance_invalid"
        | "grid_publication_approval_invalid"
        | "grid_publication_operation_summary_invalid"
        | "grid_publication_composition_invalid"
        | "grid_publication_migration_invalid"
        | "grid_publication_bump_without_head" => Failure::invalid(code, message),
        _ => Failure::failed(PREPARATION.code, message),
    };
    let refusal = COMMAND
        .refusals
        .iter()
        .find(|refusal| refusal.code == failure.code())
        .unwrap_or(&PREPARATION);
    failure.remedy(refusal.remedy)
}

fn request(inputs: &Inputs) -> Result<Request, Failure> {
    let expected = inputs
        .value("expected-head")
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            invalid(
                EXPECTED_HEAD,
                "existing MV publication requires --expected-head",
            )
        })?;
    Ok(Request {
        project: inputs.require("project")?.into(),
        model_id: Some(inputs.require("model")?.into()),
        model_kind: "mv_line".into(),
        expected_head_revision_id: Some(expected.into()),
        reason: Some(inputs.require("reason")?.into()),
        ..Default::default()
    })
}

fn read_source(path: &Path) -> Result<Vec<u8>, Failure> {
    if !path.is_absolute() || path.extension().and_then(|value| value.to_str()) != Some("dsgrid") {
        return Err(invalid(SOURCE, "--file must be an absolute .dsgrid path"));
    }
    let before = std::fs::symlink_metadata(path).map_err(|error| invalid(SOURCE, error))?;
    if !before.is_file() || before.file_type().is_symlink() {
        return Err(invalid(SOURCE, "source must be an ordinary file"));
    }
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.custom_flags(0x0020_0000); // FILE_FLAG_OPEN_REPARSE_POINT
    }
    let file = options.open(path).map_err(|error| invalid(SOURCE, error))?;
    let metadata = file.metadata().map_err(|error| invalid(SOURCE, error))?;
    if !metadata.is_file() {
        return Err(invalid(SOURCE, "opened source must be an ordinary file"));
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes() & 0x400 != 0 {
            return Err(invalid(SOURCE, "source must not be a reparse point"));
        }
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if before.dev() != metadata.dev() || before.ino() != metadata.ino() {
            return Err(invalid(SOURCE, "source changed while opening"));
        }
    }
    read_bounded(file, metadata.len())
}

fn read_bounded(file: File, length: u64) -> Result<Vec<u8>, Failure> {
    if length == 0 || length > MAX_MODEL_BYTES as u64 {
        return Err(prepare_failure(
            "model_too_large: expected a nonempty package up to 256 MiB".into(),
        ));
    }
    let mut bytes = Vec::new();
    file.take(MAX_MODEL_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| invalid(SOURCE, error))?;
    if bytes.len() > MAX_MODEL_BYTES {
        return Err(prepare_failure(
            "model_too_large: source grew beyond 256 MiB".into(),
        ));
    }
    if bytes.len() as u64 != length {
        return Err(invalid(SOURCE, "source length changed while reading"));
    }
    Ok(bytes)
}

fn verified_receipt(data: Value, plan: &Plan, lane: &str) -> Result<Value, Failure> {
    let intent = &plan.intent;
    let valid = data["status"] == "published"
        && data["verified"] == true
        && data["project"] == intent.project
        && data["project_model"] == intent.model_id
        && data["revision"] == plan.revision_id
        && data["digest"] == plan.artifact.digest
        && data["byte_length"].as_u64() == Some(plan.artifact.byte_length)
        && data["kind"] == "mv_line"
        && data["expected_head"] == intent.expected_head_revision_id
        && data["parent_revision"] == intent.expected_head_revision_id
        && data["local_revision"] == intent.source_revision_id
        && data["bump_version"] == false
        && data["version"].as_u64().is_some_and(|value| value > 0)
        && data["upload_skipped"].is_boolean()
        && (data["revision_ordinal_within_version"].is_null()
            || data["revision_ordinal_within_version"]
                .as_u64()
                .is_some_and(|value| value > 0));
    if !valid {
        return Err(Failure::failed(RECEIPT.code, "publication acknowledgement does not verify the exact prepared revision")
            .remedy(RECEIPT.remedy)
            .detail(json!({"project":intent.project,"lane":lane,"model":intent.model_id,
                "expected_head":intent.expected_head_revision_id,"prepared_revision":plan.revision_id,
                "source_sha256":plan.artifact.digest,"publication_outcome":"unconfirmed"})));
    }
    let mut result = json!({"lane":lane,"active_model_changed":false});
    for key in [
        "status",
        "project",
        "project_model",
        "revision",
        "version",
        "kind",
        "expected_head",
        "parent_revision",
        "digest",
        "byte_length",
        "upload_skipped",
        "verified",
        "local_revision",
        "bump_version",
        "milestone",
        "approval",
        "revision_ordinal_within_version",
    ] {
        if let Some(value) = data.get(key) {
            result[key] = value.clone();
        }
    }
    Ok(result)
}

fn publish_with(
    inputs: &Inputs,
    context: &Context,
    prepare: impl FnOnce(&Request, &[u8]) -> Result<Intent, String>,
    publish: impl FnOnce(&str, &str, &ds_cli_auth::GridModelsCommand) -> Result<Value, Failure>,
) -> Result<Value, Failure> {
    if !context.confirmed {
        return Err(invalid(
            CONFIRMATION,
            "publishing a governed MV revision requires --yes",
        ));
    }
    let request = request(inputs)?;
    let lane = inputs.require("lane")?;
    let bytes = read_source(Path::new(inputs.require("file")?))?;
    let intent = prepare(&request, &bytes).map_err(prepare_failure)?;
    if intent.project != request.project
        || Some(&intent.model_id) != request.model_id.as_ref()
        || intent.model_kind != "mv_line"
        || Some(&intent.expected_head_revision_id) != request.expected_head_revision_id.as_ref()
        || intent.bump_version
        || intent.display_name.is_some()
        || intent.migration_source.is_some()
        || intent.reason != request.reason
    {
        return Err(Failure::failed(
            PREPARATION.code,
            "native preparation changed the requested existing-model scope",
        )
        .remedy(PREPARATION.remedy));
    }
    let plan = intent
        .plan(&request.project, &bytes)
        .map_err(|code| prepare_failure(code.into()))?;
    let data = publish(
        lane,
        &request.project,
        &ds_cli_auth::GridModelsCommand::Publish {
            intent: Box::new(intent),
            bytes,
        },
    )?;
    // The authenticated owner already performs exact immutable readback. No second
    // history walk can turn a committed revision into an apparent failed publish.
    verified_receipt(data, &plan, lane)
}

pub fn run(inputs: &Inputs, context: &Context) -> Result<Value, Failure> {
    publish_with(
        inputs,
        context,
        grid_publication::prepare,
        |lane, project, command| {
            ds_cli_auth::grid_models_for_project(lane, project, command)
                .map(|receipt| receipt.data)
                .map_err(|failure| crate::models::native_failure(failure, project, lane))
        },
    )
}

pub fn render(data: &Value) -> String {
    serde_json::to_string_pretty(data).unwrap_or_default() + "\n"
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    fn context(confirmed: bool) -> Context {
        Context {
            confirmed,
            output: ds_cli_contract::Output::resolve(ds_cli_contract::Format::Json, false, true),
        }
    }
    fn tokens(path: &Path) -> Vec<String> {
        [
            "--project",
            "project-exact",
            "--lane",
            "canary",
            "--model",
            "mv-model",
            "--expected-head",
            "rev-old",
            "--file",
            path.to_str().unwrap(),
            "--reason",
            "Reviewed checkpoint",
        ]
        .iter()
        .map(|value| value.to_string())
        .collect()
    }
    fn inputs(path: &Path) -> Inputs {
        ds_cli_contract::parse(&COMMAND, &tokens(path)).unwrap()
    }
    fn source() -> (tempfile::TempDir, std::path::PathBuf, Vec<u8>) {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("checkpoint.dsgrid");
        let bytes = b"opaque exact package bytes".to_vec();
        std::fs::write(&path, &bytes).unwrap();
        (directory, path, bytes)
    }
    fn prepared(request: &Request, _: &[u8]) -> Result<Intent, String> {
        Ok(Intent {
            project: request.project.clone(),
            model_id: request.model_id.clone().unwrap(),
            model_kind: request.model_kind.clone(),
            expected_head_revision_id: request.expected_head_revision_id.clone().unwrap(),
            source_revision_id: "rev:new-checkpoint".into(),
            manifest_model_id: "native-model".into(),
            manifest_model_revision: 3,
            model_schema_version: "19".into(),
            engine_version: "test-engine".into(),
            reason: request.reason.clone(),
            ..Default::default()
        })
    }
    fn receipt(intent: &Intent, bytes: &[u8]) -> Value {
        let plan = intent.plan(&intent.project, bytes).unwrap();
        json!({"status":"published","project":intent.project,"project_model":intent.model_id,
            "revision":plan.revision_id,"version":2,"kind":"mv_line","verified":true,
            "expected_head":intent.expected_head_revision_id,"parent_revision":intent.expected_head_revision_id,
            "digest":plan.artifact.digest,"byte_length":bytes.len(),"local_revision":intent.source_revision_id,
            "bump_version":false,"upload_skipped":false,"revision_ordinal_within_version":4})
    }

    #[test]
    fn grammar_requires_explicit_existing_scope_and_excludes_other_workflows() {
        let (_, path, _) = source();
        let all = tokens(&path);
        for flag in [
            "--project",
            "--lane",
            "--model",
            "--expected-head",
            "--file",
            "--reason",
        ] {
            let index = all.iter().position(|value| value == flag).unwrap();
            let mut missing = all.clone();
            missing.drain(index..index + 2);
            assert!(
                ds_cli_contract::parse(&COMMAND, &missing).is_err(),
                "{flag}"
            );
        }
        for flag in [
            "--bump-version",
            "--replace-content",
            "--attach",
            "--name",
            "--kind",
            "--crs",
        ] {
            let mut extra = all.clone();
            extra.push(flag.into());
            assert!(ds_cli_contract::parse(&COMMAND, &extra).is_err(), "{flag}");
        }
        let request = request(&inputs(&path)).unwrap();
        assert_eq!(request.project, "project-exact");
        assert_eq!(request.model_id.as_deref(), Some("mv-model"));
        assert_eq!(
            request.expected_head_revision_id.as_deref(),
            Some("rev-old")
        );
        assert_eq!(request.model_kind, "mv_line");
        assert!(!request.bump_version);
        assert!(request.display_name.is_none() && request.migration_source.is_none());
        assert!(request.composition_sources.is_empty());
    }

    #[test]
    fn confirmation_and_missing_head_refuse_before_preparation_or_remote_reads() {
        let inputs = inputs(Path::new("relative.dsgrid"));
        let failure = publish_with(
            &inputs,
            &context(false),
            |_, _| panic!("no preparation"),
            |_, _, _| panic!("no publication"),
        )
        .unwrap_err();
        assert_eq!(failure.code(), CONFIRMATION.code);
        assert_eq!(
            request(&Inputs::default()).unwrap_err().code(),
            EXPECTED_HEAD.code
        );
        let mut empty = tokens(Path::new("relative.dsgrid"));
        let position = empty
            .iter()
            .position(|value| value == "--expected-head")
            .unwrap();
        empty[position + 1] = String::new();
        let inputs = ds_cli_contract::parse(&COMMAND, &empty).unwrap();
        let failure = publish_with(
            &inputs,
            &context(true),
            |_, _| panic!("no preparation"),
            |_, _, _| panic!("no publication"),
        )
        .unwrap_err();
        assert_eq!(failure.code(), EXPECTED_HEAD.code);
    }

    #[test]
    fn source_is_an_unchanged_bounded_ordinary_absolute_package() {
        let (_directory, path, bytes) = source();
        assert_eq!(read_source(&path).unwrap(), bytes);
        assert_eq!(
            read_source(Path::new("relative.dsgrid"))
                .unwrap_err()
                .code(),
            SOURCE.code
        );
        let wrong = path.with_extension("bak");
        std::fs::write(&wrong, &bytes).unwrap();
        assert_eq!(read_source(&wrong).unwrap_err().code(), SOURCE.code);
        let directory = path.with_file_name("directory.dsgrid");
        std::fs::create_dir(&directory).unwrap();
        assert_eq!(read_source(&directory).unwrap_err().code(), SOURCE.code);
        let empty = path.with_file_name("empty.dsgrid");
        File::create(&empty).unwrap();
        assert_eq!(read_source(&empty).unwrap_err().code(), "model_too_large");
        let large = path.with_file_name("large.dsgrid");
        File::create(&large)
            .unwrap()
            .set_len(MAX_MODEL_BYTES as u64 + 1)
            .unwrap();
        let failure = publish_with(
            &inputs(&large),
            &context(true),
            |_, _| panic!("bound before preparation"),
            |_, _, _| panic!("bound before publication"),
        )
        .unwrap_err();
        assert_eq!(failure.code(), "model_too_large");
        assert_eq!(
            read_bounded(File::open(&path).unwrap(), bytes.len() as u64 + 1)
                .unwrap_err()
                .code(),
            SOURCE.code
        );
        assert_eq!(std::fs::read(path).unwrap(), bytes);
    }

    #[cfg(unix)]
    #[test]
    fn symbolic_link_source_is_refused() {
        let (_directory, path, _) = source();
        let linked = path.with_file_name("linked.dsgrid");
        std::os::unix::fs::symlink(&path, &linked).unwrap();
        assert_eq!(read_source(&linked).unwrap_err().code(), SOURCE.code);
    }

    #[test]
    fn preparation_keeps_named_refusals_and_never_calls_publication_on_failure() {
        for refusal in PREPARE_REFUSALS {
            let failure = prepare_failure(format!("{}: exact owner cause", refusal.code));
            assert_eq!(failure.code(), refusal.code);
            assert!(failure.remedy_text().is_some());
        }
        assert_eq!(
            prepare_failure("network_engine_missing: absent".into()).class(),
            ds_cli_contract::ExitClass::Unavailable
        );
        assert_eq!(
            prepare_failure("model_invalid: malformed".into()).class(),
            ds_cli_contract::ExitClass::InvalidInput
        );
        assert_eq!(
            prepare_failure("unknown_owner_code: exact cause".into()).code(),
            PREPARATION.code
        );
        assert_eq!(
            prepare_failure("composite_model_protected: combined".into()).code(),
            "composite_model_protected"
        );
        let (_directory, path, _) = source();
        let failure = publish_with(
            &inputs(&path),
            &context(true),
            |_, _| Err("network_engine_missing: absent".into()),
            |_, _, _| panic!("no publication after preparation refusal"),
        )
        .unwrap_err();
        assert_eq!(failure.code(), "network_engine_missing");
    }

    #[test]
    fn exact_bytes_and_reviewed_head_reach_one_typed_publication() {
        let (_directory, path, bytes) = source();
        let calls = Cell::new(0);
        let result = publish_with(
            &inputs(&path),
            &context(true),
            |request, source| {
                assert_eq!(source, bytes);
                assert_eq!(request.reason.as_deref(), Some("Reviewed checkpoint"));
                prepared(request, source)
            },
            |lane, project, command| {
                calls.set(calls.get() + 1);
                assert_eq!((lane, project), ("canary", "project-exact"));
                let ds_cli_auth::GridModelsCommand::Publish {
                    intent,
                    bytes: uploaded,
                } = command
                else {
                    panic!("only typed Publish")
                };
                assert_eq!(uploaded, &bytes);
                assert_eq!(intent.expected_head_revision_id, "rev-old");
                let mut result = receipt(intent, uploaded);
                result["url"] = json!("private-locator");
                Ok(result)
            },
        )
        .unwrap();
        assert_eq!(calls.get(), 1);
        assert_eq!(result["revision"], "rev-new-checkpoint");
        assert_eq!(result["version"], 2);
        assert_eq!(result["revision_ordinal_within_version"], 4);
        assert_eq!(result["active_model_changed"], false);
        assert!(!result.to_string().contains("private-locator"));
        assert_eq!(std::fs::read(path).unwrap(), bytes);
    }

    #[test]
    fn concurrency_and_authentication_failures_are_preserved_without_retry() {
        let (_directory, path, _) = source();
        for (code, class) in [
            ("publish_conflict", ds_cli_contract::ExitClass::Conflict),
            (
                "headless_signed_out",
                ds_cli_contract::ExitClass::Unauthorized,
            ),
            ("auth_transient", ds_cli_contract::ExitClass::Unavailable),
        ] {
            let calls = Cell::new(0);
            let original = Failure::new(class, code, "exact owner refusal")
                .detail(json!({"http_status":409}))
                .remedy("exact owner remedy");
            let result = publish_with(&inputs(&path), &context(true), prepared, |_, _, _| {
                calls.set(calls.get() + 1);
                Err(original)
            })
            .unwrap_err();
            assert_eq!(calls.get(), 1);
            assert_eq!(result.code(), code);
            assert_eq!(result.class(), class);
            assert_eq!(result.remedy_text(), Some("exact owner remedy"));
            assert_eq!(result.detail_value(), Some(&json!({"http_status":409})));
        }
    }

    #[test]
    fn preparation_cannot_change_the_existing_target_or_start_a_new_version() {
        let (_directory, path, _) = source();
        for change in ["project", "model", "head", "kind", "bump", "rename"] {
            let failure = publish_with(
                &inputs(&path),
                &context(true),
                |request, bytes| {
                    let mut intent = prepared(request, bytes)?;
                    match change {
                        "project" => intent.project = "other".into(),
                        "model" => intent.model_id = "other".into(),
                        "head" => intent.expected_head_revision_id = "other".into(),
                        "kind" => intent.model_kind = "general".into(),
                        "bump" => intent.bump_version = true,
                        "rename" => intent.display_name = Some("new name".into()),
                        _ => unreachable!(),
                    }
                    Ok(intent)
                },
                |_, _, _| panic!("no publication after changed scope"),
            )
            .unwrap_err();
            assert_eq!(failure.code(), PREPARATION.code);
        }
    }

    #[test]
    fn receipt_mismatches_are_unconfirmed_and_do_not_invite_a_blind_retry() {
        let (_directory, path, bytes) = source();
        let request = request(&inputs(&path)).unwrap();
        let intent = prepared(&request, &bytes).unwrap();
        let plan = intent.plan(&request.project, &bytes).unwrap();
        for (key, value) in [
            ("status", json!("pending")),
            ("verified", json!(false)),
            ("project", json!("other")),
            ("project_model", json!("other")),
            ("revision", json!("other")),
            ("digest", json!("bad")),
            ("byte_length", json!(1)),
            ("kind", json!("general")),
            ("expected_head", json!("other")),
            ("parent_revision", json!("other")),
            ("local_revision", json!("other")),
            ("bump_version", json!(true)),
            ("version", json!(0)),
            ("upload_skipped", Value::Null),
            ("revision_ordinal_within_version", json!(0)),
        ] {
            let mut data = receipt(&intent, &bytes);
            data[key] = value;
            let failure = verified_receipt(data, &plan, "canary").unwrap_err();
            assert_eq!(failure.code(), RECEIPT.code, "{key}");
            assert_eq!(
                failure.detail_value().unwrap()["publication_outcome"],
                "unconfirmed"
            );
            assert_eq!(
                failure.detail_value().unwrap()["source_sha256"],
                plan.artifact.digest
            );
            assert!(failure.remedy_text().unwrap().contains("Do not repeat"));
        }
        let mut valid = receipt(&intent, &bytes);
        valid["revision_ordinal_within_version"] = Value::Null;
        let committed = verified_receipt(valid, &plan, "canary").unwrap();
        assert_eq!(committed["status"], "published");
        assert!(committed["revision_ordinal_within_version"].is_null());
        assert!(committed.get("version_revision_count").is_none());
    }

    #[test]
    fn descriptor_declares_a_confirmed_governed_project_write() {
        assert!(
            crate::DOMAIN
                .commands
                .iter()
                .any(|command| command.id == COMMAND.id)
        );
        let descriptor = ds_cli_contract::help::command_json_unchecked(&COMMAND);
        assert_eq!(descriptor["confirmation_required"], true);
        assert_eq!(descriptor["effect"], "global_write");
        assert_eq!(COMMAND.authority, Authority::HeadlessProject);
        assert_eq!(COMMAND.chapter, Chapter::Design);
        // The existing Core context-budget contract scales descriptors by declared fields.
        let ceiling = 1_200 + 180 * COMMAND.args.len() + 220 * COMMAND.refusals.len() + 600;
        assert!(serde_json::to_vec(&descriptor).unwrap().len() <= ceiling);
        assert!(
            COMMAND
                .refusals
                .iter()
                .all(|refusal| !refusal.remedy.is_empty())
        );
        for refusal in PREPARE_REFUSALS {
            assert!(
                COMMAND
                    .refusals
                    .iter()
                    .any(|listed| listed.code == refusal.code)
            );
        }
    }
}
