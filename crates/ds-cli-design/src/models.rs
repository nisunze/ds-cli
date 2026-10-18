//! Read saved MV heads and acquire one immutable package through the native owner.

use std::io::Write;
use std::path::Path;

use ds_cli_contract::spec::{
    Arg, Authority, Chapter, Command, Effect, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Failure, Inputs};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

const INPUT: Refusal = Refusal {
    code: "design_model_input_invalid",
    when: "the page limit is outside 1..100",
    remedy: "Use --limit 1 through 100 and the exact next_cursor from the previous page.",
};
const OUTPUT: Refusal = Refusal {
    code: "design_model_output_invalid",
    when: "the destination is not a fresh absolute .dsgrid path in a writable directory",
    remedy: "Choose a new absolute .dsgrid filename with an existing writable parent directory.",
};
const RECEIPT: Refusal = Refusal {
    code: "design_model_receipt_invalid",
    when: "the owner receipt lacks verified exact revision bytes or a complete bounded catalog page",
    remedy: "Retry the exact project read; never use an unverified or incomplete package.",
};
const MISSING: Refusal = Refusal {
    code: "grid_model_not_found",
    when: "the exact project has no readable saved model or revision with these IDs",
    remedy: "Read active saved heads with ds design model list --project <exact-id> --lane <stable|canary>, then use the exact model and revision IDs.",
};
const SHARED: usize =
    ds_cli_auth::GRID_MODEL_REFUSALS.len() + ds_cli_auth::PROJECT_STATUS_COMMAND.refusals.len();
const fn refusals() -> [Refusal; SHARED + 3] {
    let mut rows = [INPUT; SHARED + 3];
    rows[1] = OUTPUT;
    rows[2] = RECEIPT;
    let mut index = 0;
    while index < ds_cli_auth::GRID_MODEL_REFUSALS.len() {
        // Adapt only the missing-source remedy from the retired model command family.
        rows[index + 3] = if index == 1 {
            MISSING
        } else {
            ds_cli_auth::GRID_MODEL_REFUSALS[index]
        };
        index += 1;
    }
    let mut project = 0;
    while project < ds_cli_auth::PROJECT_STATUS_COMMAND.refusals.len() {
        rows[index + project + 3] = ds_cli_auth::PROJECT_STATUS_COMMAND.refusals[project];
        project += 1;
    }
    rows
}
const REFUSALS: &[Refusal] = &refusals();

pub static LIST: Command = Command {
    id: "design.model.list",
    path: &["design", "model", "list"],
    contract: 1,
    summary: "List one project's active saved MV models for printing.",
    purpose: "Read one bounded page of active, nondeleted saved MV model heads under the exact project and native credential lane. Each head names its immutable revision and package digest for design model export and report plan-profile. Follow next_cursor whenever more is true, including an empty page; unpublished local drafts are outside this catalog.",
    chapter: Chapter::Design,
    effect: Effect::LocalAuthState,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[
        crate::PROJECT_ARG,
        crate::LANE_ARG,
        Arg::value(
            "limit",
            "<1..100>",
            "Maximum catalog rows scanned in this page.",
        )
        .default("100"),
        Arg::value(
            "cursor",
            "<opaque>",
            "Exact next_cursor from the preceding page.",
        ),
    ],
    output: "project, lane, models[] with exact model_id/head_revision_id/head_model_digest, more and next_cursor. Active MV heads only; an empty page may still have more=true.",
    examples: &[],
    refusals: REFUSALS,
    reference: Some("docs/reference/design.md"),
    search: &["saved MV models", "plan profile source"],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};

pub static EXPORT: Command = Command {
    id: "design.model.export",
    path: &["design", "model", "export"],
    contract: 1,
    summary: "Export one exact saved MV revision as verified .dsgrid bytes.",
    purpose: "Acquire one immutable model revision through the authenticated project owner for report plan-profile or other local consumers. Require the exact project, lane, model and revision; verify the owner's receipt, SHA-256 and size before atomically creating a fresh absolute .dsgrid file. Existing files are never replaced. No model conversion or engineering operation runs, and no signed URL or credential is returned.",
    chapter: Chapter::Design,
    effect: Effect::LocalFileWrite,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[
        crate::PROJECT_ARG,
        crate::LANE_ARG,
        Arg::value("model", "<id>", "Exact model_id from design model list.").required(),
        Arg::value(
            "revision",
            "<id>",
            "Exact immutable revision ID; no latest alias.",
        )
        .required(),
        Arg::value(
            "out",
            "<absolute.dsgrid>",
            "Fresh absolute .dsgrid path in an existing writable directory.",
        )
        .required(),
    ],
    output: "project, lane, model_id, revision_id, sha256, bytes, verified=true and out. No signed locator.",
    examples: &[],
    refusals: REFUSALS,
    reference: Some("docs/reference/design.md"),
    search: &["download MV model", "plan profile source"],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};

fn invalid(refusal: Refusal, why: impl std::fmt::Display) -> Failure {
    let message = why.to_string();
    let failure = match refusal.code {
        "design_model_input_invalid"
        | "design_model_output_invalid"
        | "design_model_receipt_invalid" => Failure::invalid(refusal.code, message),
        _ => Failure::failed("design_model_receipt_invalid", message),
    };
    failure.remedy(refusal.remedy)
}

pub(super) fn native_failure(failure: Failure, project: &str, lane: &str) -> Failure {
    if failure.code() != MISSING.code {
        return failure;
    }
    let mut result = Failure::new(failure.class(), "grid_model_not_found", failure.message())
        .remedy(MISSING.remedy)
        .next(format!(
            "ds design model list --project {project} --lane {lane}"
        ));
    if let Some(detail) = failure.detail_value() {
        result = result.detail(detail.clone());
    }
    result
}

fn list_request(inputs: &Inputs) -> Result<ds_cli_auth::GridModelsCommand, Failure> {
    let limit = inputs
        .require("limit")?
        .parse::<u16>()
        .ok()
        .filter(|limit| (1..=100).contains(limit))
        .ok_or_else(|| invalid(INPUT, "--limit must be 1 through 100"))?;
    Ok(ds_cli_auth::GridModelsCommand::List {
        limit,
        cursor: inputs.value("cursor").map(str::to_owned),
        include_deleted: false,
    })
}

fn active_page(data: Value, limit: u16, project: &str) -> Result<Value, Failure> {
    let more = data["more"]
        .as_bool()
        .ok_or_else(|| invalid(RECEIPT, "catalog lacks more"))?;
    let cursor = data["next_cursor"]
        .as_str()
        .ok_or_else(|| invalid(RECEIPT, "catalog lacks next_cursor"))?;
    if data["project"] != project || (more && cursor.is_empty()) || cursor.len() > 4096 {
        return Err(invalid(
            RECEIPT,
            "catalog scope or pagination is incomplete",
        ));
    }
    let rows = data["models"]
        .as_array()
        .ok_or_else(|| invalid(RECEIPT, "catalog lacks models"))?;
    if rows.len() > usize::from(limit) {
        return Err(invalid(RECEIPT, "catalog exceeds the requested page limit"));
    }
    let models = rows
        .iter()
        .filter(|row| {
            row["model_kind"] == "mv_line"
                && row["state"] == "active"
                && row["deleted_at"].is_null()
        })
        .map(|row| {
            for key in ["model_id", "head_revision_id", "head_model_digest"] {
                if !row[key].as_str().is_some_and(|value| !value.is_empty()) {
                    return Err(invalid(RECEIPT, format!("active model lacks {key}")));
                }
            }
            // The acquisition consumer needs head metadata, never retirement locators.
            let mut head = json!({});
            for key in [
                "model_id",
                "display_name",
                "model_kind",
                "state",
                "head_revision_id",
                "head_model_digest",
                "head_version",
                "updated_at",
                "approval_status",
            ] {
                if let Some(value) = row.get(key) {
                    head[key] = value.clone();
                }
            }
            Ok(head)
        })
        .collect::<Result<Vec<_>, Failure>>()?;
    Ok(json!({"project":project,"models":models,"more":more,"next_cursor":cursor}))
}

pub fn list(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
    let command = list_request(inputs)?;
    let ds_cli_auth::GridModelsCommand::List { limit, .. } = &command else {
        unreachable!()
    };
    let project = inputs.require("project")?;
    let lane = inputs.require("lane")?;
    let response = ds_cli_auth::grid_models_for_project(lane, project, &command)
        .map_err(|failure| native_failure(failure, project, lane))?;
    let mut result = active_page(response.data, *limit, project)?;
    result["lane"] = json!(lane);
    Ok(result)
}

fn admit_output(out: &Path) -> Result<tempfile::NamedTempFile, Failure> {
    if !out.is_absolute() || out.extension().and_then(|s| s.to_str()) != Some("dsgrid") {
        return Err(invalid(OUTPUT, "--out must be an absolute .dsgrid path"));
    }
    match std::fs::symlink_metadata(out) {
        Ok(_) => return Err(invalid(OUTPUT, "destination already exists")),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(invalid(OUTPUT, error)),
    }
    // Prove parent existence and writability before authorizing remote bytes.
    let parent = out
        .parent()
        .ok_or_else(|| invalid(OUTPUT, "destination lacks a parent"))?;
    tempfile::NamedTempFile::new_in(parent).map_err(|error| invalid(OUTPUT, error))
}

fn persist_verified(
    mut staged: tempfile::NamedTempFile,
    out: &Path,
    data: &Value,
    bytes: Option<&[u8]>,
    project: &str,
    model: &str,
    revision: &str,
) -> Result<Value, Failure> {
    let bytes = bytes.ok_or_else(|| invalid(RECEIPT, "owner returned no package bytes"))?;
    let digest = format!("{:x}", Sha256::digest(bytes));
    if data["verified"] != true
        || data["project"] != project
        || data["model_id"] != model
        || data["revision_id"] != revision
        || data["sha256"] != digest
        || data["bytes"].as_u64() != Some(bytes.len() as u64)
        || bytes.is_empty()
    {
        return Err(invalid(
            RECEIPT,
            "owner receipt does not verify the exact package",
        ));
    }
    staged
        .write_all(bytes)
        .map_err(|error| invalid(OUTPUT, error))?;
    staged
        .as_file()
        .sync_all()
        .map_err(|error| invalid(OUTPUT, error))?;
    staged
        .persist_noclobber(out)
        .map_err(|error| invalid(OUTPUT, error.error))?;
    // Construct a closed receipt instead of forwarding any future owner locators.
    Ok(
        json!({"project":project,"model_id":model,"revision_id":revision,
        "sha256":digest,"bytes":bytes.len(),"verified":true,"out":out}),
    )
}

pub fn export(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
    let out = Path::new(inputs.require("out")?);
    let staged = admit_output(out)?;
    let project = inputs.require("project")?;
    let lane = inputs.require("lane")?;
    let model = inputs.require("model")?;
    let revision = inputs.require("revision")?;
    let response = ds_cli_auth::grid_models_for_project(
        lane,
        project,
        &ds_cli_auth::GridModelsCommand::Download {
            model: model.into(),
            revision: revision.into(),
        },
    )
    .map_err(|failure| native_failure(failure, project, lane))?;
    let mut result = persist_verified(
        staged,
        out,
        &response.data,
        response.bytes.as_deref(),
        project,
        model,
        revision,
    )?;
    result["lane"] = json!(lane);
    Ok(result)
}

pub fn render(data: &Value) -> String {
    serde_json::to_string_pretty(data).unwrap_or_default() + "\n"
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parsed(command: &Command, tokens: &[&str]) -> Result<Inputs, Failure> {
        ds_cli_contract::parse(
            command,
            &tokens.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
        )
    }
    fn receipt(bytes: &[u8]) -> Value {
        json!({"project":"project","model_id":"model","revision_id":"revision",
            "sha256":format!("{:x}", Sha256::digest(bytes)),"bytes":bytes.len(),"verified":true})
    }
    #[test]
    fn grammar_requires_exact_scope_and_revision_and_limits_pages() {
        assert!(parsed(&LIST, &[]).is_err());
        assert!(
            parsed(
                &EXPORT,
                &[
                    "--project",
                    "project",
                    "--model",
                    "model",
                    "--out",
                    "a.dsgrid"
                ]
            )
            .is_err()
        );
        assert!(parsed(&LIST, &["--project", "project", "--include-deleted"]).is_err());
        let inputs = parsed(&LIST, &["--project", "project"]).unwrap();
        assert_eq!(inputs.require("lane").unwrap(), "stable");
        assert!(matches!(
            list_request(&inputs).unwrap(),
            ds_cli_auth::GridModelsCommand::List {
                limit: 100,
                include_deleted: false,
                cursor: None
            }
        ));
        for bound in ["0", "101", "-1", "65536", "wrong"] {
            let inputs = parsed(&LIST, &["--project", "project", "--limit", bound]).unwrap();
            assert_eq!(list_request(&inputs).err().unwrap().code(), INPUT.code);
        }
        let inputs = parsed(
            &LIST,
            &[
                "--project",
                "project",
                "--lane",
                "canary",
                "--limit",
                "1",
                "--cursor",
                "opaque",
            ],
        )
        .unwrap();
        assert!(
            matches!(list_request(&inputs).unwrap(), ds_cli_auth::GridModelsCommand::List { limit: 1, cursor: Some(cursor), .. } if cursor == "opaque")
        );
    }
    #[test]
    fn empty_filtered_page_keeps_the_owner_cursor_and_bounds() {
        let page = json!({"project":"project","models":[{"model_kind":"mv_line","state":"deleted"}],"more":true,"next_cursor":"opaque"});
        let result = active_page(page, 1, "project").unwrap();
        assert_eq!(result["models"], json!([]));
        assert_eq!(result["next_cursor"], "opaque");
        assert_eq!(result["more"], true);
        assert!(active_page(result.clone(), 1, "other").is_err());
        let mut invalid = result;
        invalid["next_cursor"] = json!("");
        assert!(active_page(invalid, 1, "project").is_err());
        assert!(
            active_page(
                json!({"project":"project","models":[{},{}],"more":false,"next_cursor":""}),
                1,
                "project"
            )
            .is_err()
        );
    }
    #[test]
    fn active_heads_preserve_exact_identity_and_exclude_retirement_locators() {
        let page = json!({"project":"project","models":[
            {"model_kind":"mv_line","state":"active","model_id":"model-1","head_revision_id":"revision-1","head_model_digest":"digest","backup":{"url":"private-locator"}},
            {"model_kind":"mv_line","state":"active","deleted_at":"time"},
            {"model_kind":"other","state":"active"}
        ],"more":false,"next_cursor":""});
        let result = active_page(page, 3, "project").unwrap();
        assert_eq!(result["models"].as_array().unwrap().len(), 1);
        assert_eq!(result["models"][0]["model_id"], "model-1");
        assert_eq!(result["models"][0]["head_revision_id"], "revision-1");
        assert!(!result.to_string().contains("private-locator"));
        assert!(active_page(json!({"project":"project","models":[{"model_kind":"mv_line","state":"active"}],"more":false,"next_cursor":""}), 1, "project").is_err());
    }
    #[test]
    fn output_admission_precedes_acquisition_and_rejects_existing_paths() {
        assert!(admit_output(Path::new("relative.dsgrid")).is_err());
        let root = tempfile::tempdir().unwrap();
        assert!(admit_output(&root.path().join("model.json")).is_err());
        assert!(admit_output(&root.path().join("absent").join("model.dsgrid")).is_err());
        let out = root.path().join("model.dsgrid");
        std::fs::write(&out, b"existing").unwrap();
        assert!(admit_output(&out).is_err());
        assert_eq!(std::fs::read(out).unwrap(), b"existing");
    }
    #[test]
    fn public_handlers_refuse_invalid_local_inputs_without_authentication() {
        let context = Context {
            confirmed: false,
            output: ds_cli_contract::Output::resolve(ds_cli_contract::Format::Json, false, true),
        };
        let inputs = parsed(&LIST, &["--project", "project", "--limit", "101"]).unwrap();
        assert_eq!(list(&inputs, &context).unwrap_err().code(), INPUT.code);
        let inputs = parsed(
            &EXPORT,
            &[
                "--project",
                "project",
                "--model",
                "model",
                "--revision",
                "revision",
                "--out",
                "relative.dsgrid",
            ],
        )
        .unwrap();
        assert_eq!(export(&inputs, &context).unwrap_err().code(), OUTPUT.code);
    }
    #[test]
    fn verified_bytes_are_exact_and_racing_destination_is_preserved() {
        let root = tempfile::tempdir().unwrap();
        let out = root.path().join("model.dsgrid");
        let bytes = b"immutable package";
        let data = receipt(bytes);
        let result = persist_verified(
            admit_output(&out).unwrap(),
            &out,
            &data,
            Some(bytes),
            "project",
            "model",
            "revision",
        )
        .unwrap();
        assert_eq!(std::fs::read(&out).unwrap(), bytes);
        assert_eq!(result["sha256"], data["sha256"]);
        let race = root.path().join("race.dsgrid");
        let staged = admit_output(&race).unwrap();
        std::fs::write(&race, b"racing writer").unwrap();
        assert_eq!(
            persist_verified(
                staged,
                &race,
                &data,
                Some(bytes),
                "project",
                "model",
                "revision"
            )
            .unwrap_err()
            .code(),
            OUTPUT.code
        );
        assert_eq!(std::fs::read(&race).unwrap(), b"racing writer");
        assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 2);
    }
    #[test]
    fn unverified_digest_size_or_identity_failures_leave_no_file() {
        let root = tempfile::tempdir().unwrap();
        let out = root.path().join("model.dsgrid");
        let bytes = b"package";
        for (key, value) in [
            ("verified", json!(false)),
            ("sha256", json!("bad")),
            ("bytes", json!(1)),
            ("project", json!("other")),
            ("model_id", json!("other")),
            ("revision_id", json!("other")),
        ] {
            let mut data = receipt(bytes);
            data[key] = value;
            assert_eq!(
                persist_verified(
                    admit_output(&out).unwrap(),
                    &out,
                    &data,
                    Some(bytes),
                    "project",
                    "model",
                    "revision"
                )
                .unwrap_err()
                .code(),
                RECEIPT.code
            );
            assert!(!out.exists());
            assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 0);
        }
        assert!(
            persist_verified(
                admit_output(&out).unwrap(),
                &out,
                &receipt(bytes),
                None,
                "project",
                "model",
                "revision"
            )
            .is_err()
        );
    }
    #[test]
    fn catalog_declares_only_the_two_bounded_read_capabilities() {
        for command in [&LIST, &EXPORT] {
            assert!(
                crate::DOMAIN
                    .commands
                    .iter()
                    .any(|listed| listed.id == command.id)
            );
            assert_eq!(command.chapter, Chapter::Design);
            assert_eq!(command.authority, Authority::HeadlessProject);
            assert!(command.refusals.iter().all(|row| !row.remedy.is_empty()));
            let descriptor = ds_cli_contract::help::command_json_unchecked(command);
            assert_eq!(descriptor["id"], command.id);
            assert_eq!(descriptor["chapter"], "design");
            assert_eq!(descriptor["confirmation_required"], false);
        }
        assert_eq!(LIST.path, &["design", "model", "list"]);
        assert_eq!(EXPORT.path, &["design", "model", "export"]);
        assert!(!EXPORT.args.iter().any(|arg| arg.name == "url"));
    }
    #[test]
    fn missing_source_points_to_the_live_exact_project_and_lane_read() {
        assert_eq!(ds_cli_auth::GRID_MODEL_REFUSALS[1].code, MISSING.code);
        let original = Failure::invalid("grid_model_not_found", "no such revision")
            .detail(json!({"http_status":404}))
            .next("ds dsgrid project list --include-deleted");
        let result = native_failure(original, "project-exact", "canary");
        assert_eq!(result.code(), MISSING.code);
        assert_eq!(result.message(), "no such revision");
        assert_eq!(result.detail_value(), Some(&json!({"http_status":404})));
        assert_eq!(
            result.next_commands(),
            &["ds design model list --project project-exact --lane canary"]
        );
        assert_eq!(
            LIST.refusals
                .iter()
                .find(|row| row.code == MISSING.code)
                .unwrap()
                .remedy,
            MISSING.remedy
        );
        let other = native_failure(
            Failure::unavailable("auth_transient", "offline"),
            "project",
            "stable",
        );
        assert_eq!(other.code(), "auth_transient");
    }
}
