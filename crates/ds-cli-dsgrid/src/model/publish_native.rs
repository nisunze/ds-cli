//! Native file publication, including governed replacement-content import.
use ds_cli_contract::{Inputs, outcome::Failure};
use ds_command_kernel::grid_publication::{Approval, CompositionSource, MigrationSource};
use ds_design_workspace::grid_publication::{self, Request};
use ds_grid_exchange::conversion::{
    BatchMode, ConversionRequest, OutcomeStatus, PlsContainer, PlsVersionIntent, SourceCandidate,
    SourceSet, TargetFormat, execute_conversion, plan_conversion, resolve_pls_project_selection,
};
use ds_grid_exchange::lineage_transfer::{
    ImportAsVersionError, ImportAsVersionRequest, import_package_as_version,
};
use ds_grid_exchange::package::unpack;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::path::Path;

/// Most bytes one `--attach` file may carry (the attachment owner's bound).
const MAX_ATTACHMENT_BYTES: u64 = 512 * 1024 * 1024;

pub fn run(inputs: &Inputs, path: &str) -> Result<Value, Failure> {
    let project = inputs.require("project")?;
    let lane = inputs.value("lane").unwrap_or("stable");
    let replace_content = inputs.switch("replace-content");
    // Everything a caller named is checked before anything is uploaded: an
    // unreadable attachment must not leave a revision behind it.
    let attachments = attachments(inputs)?;
    let mut request = Request {
        project: project.into(),
        model_id: inputs.value("project-model").map(str::to_owned),
        model_kind: inputs.require("kind")?.into(),
        expected_head_revision_id: inputs.value("expected-head").map(str::to_owned),
        display_name: inputs.value("name").map(str::to_owned),
        reason: inputs.value("reason").map(str::to_owned),
        bump_version: inputs.switch("bump-version"),
        milestone: inputs.value("milestone").map(str::to_owned),
        description: inputs.value("description").map(str::to_owned),
        design_stage_id: inputs.value("design-stage").map(str::to_owned),
        detail_level_id: inputs.value("detail-level").map(str::to_owned),
        approval: approval(inputs)?,
        operation_summary: inputs.repeated("operation-summary").to_vec(),
        composition_sources: composition_sources(inputs, lane, project)?,
        migration_source: None,
    };
    let (bytes, import_receipt, source_receipt, migration) = if replace_content {
        let (incoming, source_receipt, raw_digest) = incoming_bytes(inputs, path)?;
        let model = inputs.require("project-model")?;
        let revision = inputs.require("expected-head")?;
        let downloaded = ds_cli_auth::grid_models_for_project(
            lane,
            project,
            &ds_cli_auth::GridModelsCommand::Download {
                model: model.into(),
                revision: revision.into(),
            },
        )?;
        let target_bytes = downloaded.bytes.ok_or_else(|| {
            Failure::failed(
                "replace_content_invalid",
                "verified head download returned no bytes",
            )
        })?;
        let incoming_package = unpack(&incoming).map_err(|e| {
            Failure::invalid("replace_content_invalid", format!("incoming package: {e}"))
        })?;
        let target_package = unpack(&target_bytes).map_err(|e| {
            Failure::invalid(
                "replace_content_invalid",
                format!("project head package: {e}"),
            )
        })?;
        let incoming_sha = format!("{:x}", Sha256::digest(&incoming));
        let target_sha = downloaded.data["sha256"].as_str().ok_or_else(|| {
            Failure::failed(
                "replace_content_invalid",
                "verified head download has no digest",
            )
        })?;
        let imported = import_package_as_version(&ImportAsVersionRequest {
            working_bytes: &incoming,
            target_head_bytes: &target_bytes,
            expected_working_sha256: &incoming_sha,
            expected_target_head_sha256: target_sha,
            expected_working_model_id: &incoming_package.manifest.model.model_id,
            expected_target_model_id: &target_package.manifest.model.model_id,
            expected_target_revision: target_package.manifest.model.model_revision,
            libraries: &[],
        })
        .map_err(import_failure)?;
        let backup = Path::new(path)
            .extension()
            .and_then(|x| x.to_str())
            .is_some_and(|x| x.eq_ignore_ascii_case("bak"));
        let migration = migration(
            path,
            backup,
            &raw_digest,
            incoming_package.manifest.model.model_id.as_str(),
        );
        (
            imported.package_bytes,
            Some(json!(imported.receipt)),
            source_receipt,
            migration,
        )
    } else {
        if inputs.value("crs").is_some()
            || inputs.value("select-project").is_some()
            || inputs.switch("swap-xy")
        {
            return Err(Failure::invalid(
                "replace_content_target_required",
                "native import options require --replace-content",
            ));
        }
        let bytes = crate::package::read_bytes(path)?;
        // A NEW model's first revision is where the catalog admits typed
        // migration evidence; a package converted from PLS-CADD names the
        // exact workspace it was read from.
        let migration = if inputs.value("project-model").is_none() {
            workspace_origin(&bytes)
        } else {
            None
        };
        (bytes, None, None, migration)
    };
    // Typed migration evidence is admissible on a model's first revision
    // only; a later revision states the same lineage as one summary line.
    if let Some(migration) = migration {
        if request.model_id.is_none() {
            request.migration_source = Some(migration);
        } else {
            request
                .operation_summary
                .push(ds_command_kernel::grid_publication::migration_summary(
                    &migration,
                ));
        }
    }
    let intent = grid_publication::prepare(&request, &bytes).map_err(|message| {
        let failure = match message.split(':').next().unwrap_or("") {
            "publish_expected_head_required" => {
                Failure::invalid("publish_expected_head_required", message)
            }
            "model_too_large" => Failure::invalid("model_too_large", message),
            code if code.starts_with("grid_publication_")
                && code != "grid_publication_project_or_model_invalid"
                && code != "grid_publication_source_invalid"
                && code != "grid_publication_revision_invalid" =>
            {
                Failure::invalid("publish_governance_invalid", message)
            }
            _ => Failure::invalid("model_invalid", message),
        };
        failure.remedy("inspect the package and exact publication inputs before retrying")
    })?;
    let model = intent.model_id.clone();
    let mut receipt = ds_cli_auth::grid_models_for_project(
        lane,
        project,
        &ds_cli_auth::GridModelsCommand::Publish {
            intent: Box::new(intent),
            bytes,
        },
    )?
    .data;
    if let Some(import) = import_receipt {
        receipt["content_import"] = import;
        receipt["active_model_changed"] = json!(false);
    }
    if let Some(source) = source_receipt {
        receipt["source_conversion"] = source;
    }
    let revision = receipt["revision"].as_str().unwrap_or_default().to_owned();
    let revision_digest = receipt["digest"].as_str().map(str::to_owned);
    position(&mut receipt, lane, project, &model, &revision);
    if !attachments.is_empty() {
        attach(
            &mut receipt,
            lane,
            project,
            &model,
            &revision,
            revision_digest.as_deref(),
            attachments,
        );
    }
    Ok(receipt)
}

/// Where the committed revision sits in its version. A Server that stores
/// ordinals said so in the commit (and the revision is then the head, so its
/// ordinal is the version's count); an older one is walked. The revision is
/// already committed; a walk that fails is reported beside it, never raised.
fn position(receipt: &mut Value, lane: &str, project: &str, model: &str, revision: &str) {
    if let Some(ordinal) = receipt["revision_ordinal_within_version"].as_u64() {
        receipt["version_started"] = json!(ordinal == 1);
        receipt["version_revision_count"] = json!(ordinal);
        receipt["count_exact"] = json!(true);
        return;
    }
    match ds_cli_auth::grid_models_for_project(
        lane,
        project,
        &ds_cli_auth::GridModelsCommand::Position {
            model: model.into(),
            revision: revision.into(),
        },
    ) {
        Ok(position) => {
            let ordinal = position.data["revision_ordinal_within_version"].clone();
            // Unknown, not false, when the walk could not reach the version's
            // first revision.
            receipt["version_started"] = if ordinal.is_null() {
                Value::Null
            } else {
                json!(ordinal == json!(1))
            };
            receipt["revision_ordinal_within_version"] = ordinal;
            receipt["version_revision_count"] = position.data["version_revision_count"].clone();
            receipt["count_exact"] = position.data["count_exact"].clone();
        }
        Err(error) => {
            receipt["revision_ordinal_within_version"] = Value::Null;
            receipt["position_unavailable"] =
                json!({"code": error.code(), "message": error.to_string()});
        }
    }
}

/// One `--attach` file, read and bounded before publication.
struct Attachment {
    path: String,
    file: String,
    purpose: Option<String>,
    bytes: Vec<u8>,
}

/// `<path>[:purpose]`. A suffix counts as a purpose only when it is a plain
/// token and the whole argument is not itself an existing file, so a path
/// that contains `:` (or a Windows drive) is never cut.
fn split_attach(raw: &str) -> (String, Option<String>) {
    if Path::new(raw).is_file() {
        return (raw.to_owned(), None);
    }
    match raw.rsplit_once(':') {
        Some((path, purpose))
            if !path.is_empty()
                && (1..=64).contains(&purpose.len())
                && purpose
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-')) =>
        {
            (path.to_owned(), Some(purpose.to_owned()))
        }
        _ => (raw.to_owned(), None),
    }
}

fn attachments(inputs: &Inputs) -> Result<Vec<Attachment>, Failure> {
    inputs
        .repeated("attach")
        .iter()
        .map(|raw| {
            let (path, purpose) = split_attach(raw);
            let refuse = |why: String| {
                Failure::invalid("attachment_file_invalid", format!("--attach {raw}: {why}"))
                    .remedy("name existing files; each is checked before anything is published")
            };
            let length = std::fs::metadata(&path)
                .map_err(|e| refuse(e.to_string()))?
                .len();
            if length == 0 || length > MAX_ATTACHMENT_BYTES {
                return Err(refuse(format!("holds {length} bytes")));
            }
            let file = Path::new(&path)
                .file_name()
                .and_then(|name| name.to_str())
                .filter(|name| name.len() <= 240)
                .ok_or_else(|| refuse("needs a UTF-8 file name of at most 240 bytes".into()))?
                .to_owned();
            let bytes = std::fs::read(&path).map_err(|e| refuse(e.to_string()))?;
            Ok(Attachment {
                path,
                file,
                purpose,
                bytes,
            })
        })
        .collect()
}

/// Attach each file to the NEW revision. The revision stands whatever
/// happens here; every file is reported, and one that failed is named with
/// its code so it can be attached again with `ds design attachment publish`.
fn attach(
    receipt: &mut Value,
    lane: &str,
    project: &str,
    model: &str,
    revision: &str,
    revision_digest: Option<&str>,
    files: Vec<Attachment>,
) {
    let mut reported = Vec::with_capacity(files.len());
    for file in files {
        let command = ds_cli_auth::DesignAttachmentsCommand::Publish {
            object: ds_cli_auth::DesignAttachmentObject {
                kind: "mv_model".into(),
                id: model.into(),
                version: Some(revision.into()),
                version_digest: revision_digest.map(str::to_owned),
            },
            attachment: None,
            file: file.file.clone(),
            label: None,
            purpose: file.purpose.clone(),
            media_type: None,
            source_kind: None,
            source_reference: None,
            make_latest: true,
            bytes: file.bytes,
        };
        reported.push(
            match ds_cli_auth::design_attachments_for_project(lane, project, &command) {
                Ok(done) => json!({"file":file.file,"path":file.path,"purpose":file.purpose,
                    "attachment":done["attachment"],"revision":done["revision"],
                    "bytes":done["bytes"],"digest":done["digest"],"verified":done["verified"]}),
                Err(error) => json!({"file":file.file,"path":file.path,"purpose":file.purpose,
                    "error":error.code(),"message":error.to_string(),
                    "retry":format!("ds design attachment publish --project {project} --kind mv_model --object {model} --version {revision} --path <file> --yes")}),
            },
        );
    }
    let failed = reported.iter().filter(|r| r.get("error").is_some()).count();
    receipt["attachments"] = json!(reported);
    receipt["attachments_failed"] = json!(failed);
}

fn approval(inputs: &Inputs) -> Result<Option<Approval>, Failure> {
    match inputs.value("approval") {
        Some(status) => Ok(Some(Approval {
            status: status.into(),
            level_id: inputs.value("approval-level").map(str::to_owned),
            decision_reason: inputs.value("approval-reason").map(str::to_owned),
        })),
        None if inputs.value("approval-level").is_some()
            || inputs.value("approval-reason").is_some() =>
        {
            Err(Failure::invalid(
                "publish_governance_invalid",
                "--approval-level and --approval-reason need --approval",
            )
            .remedy("name the review state with --approval"))
        }
        None => Ok(None),
    }
}

/// Each `model:revision` is resolved to the digest the catalog holds for it;
/// the catalog then checks the same revisions exist in this project.
fn composition_sources(
    inputs: &Inputs,
    lane: &str,
    project: &str,
) -> Result<Vec<CompositionSource>, Failure> {
    inputs
        .repeated("composition-source")
        .iter()
        .enumerate()
        .map(|(sequence, raw)| {
            let (model, revision) = raw
                .split_once(':')
                .filter(|(m, r)| !m.is_empty() && !r.is_empty())
                .ok_or_else(|| {
                    Failure::invalid(
                        "publish_governance_invalid",
                        format!("--composition-source `{raw}` is not model:revision"),
                    )
                    .remedy("name each source as <project-model-id>:<revision-id>")
                })?;
            let shown = ds_cli_auth::grid_models_for_project(
                lane,
                project,
                &ds_cli_auth::GridModelsCommand::ShowVersion {
                    model: model.into(),
                    revision: revision.into(),
                },
            )?;
            Ok(CompositionSource {
                sequence: sequence as u64,
                source_kind: "project_model".into(),
                model_id: model.into(),
                revision_id: Some(revision.into()),
                model_digest: shown.data["revision"]["model"]["digest"]
                    .as_str()
                    .unwrap_or_default()
                    .to_owned(),
            })
        })
        .collect()
}

/// Provenance of converted or imported content: the kind, the file name
/// (never the local directory), the source bytes' digest and the manifest
/// identity it produced, spelled in the catalog's identifier grammar.
/// The whole-workspace origin a converted package records about itself: the
/// source system's preserved bundle, its exact digest and the manifest it
/// produced. `None` for a package with no such record (authored in DS, or
/// predating origin records), which is published without migration evidence.
fn workspace_origin(bytes: &[u8]) -> Option<MigrationSource> {
    use ds_grid_exchange::origin::{
        ORIGIN_AUTHORITIES_LEAF, OriginAuthorityScope, decode_origin_authorities,
    };
    let package = unpack(bytes).ok()?;
    let registry = package
        .assets
        .iter()
        .find(|asset| asset.invariant_leaf == ORIGIN_AUTHORITIES_LEAF)
        .and_then(|asset| decode_origin_authorities(&asset.bytes).ok())?;
    let record = registry
        .records
        .iter()
        .find(|record| record.scope == OriginAuthorityScope::WorkspaceBundle)?;
    let digest = record.content_digest.strip_prefix("sha256:")?;
    Some(MigrationSource {
        kind: catalog_token(&format!("{}_workspace", record.source_system)),
        reference: record.source_leaf.clone(),
        source_digest: digest.to_owned(),
        manifest_id: catalog_token(package.manifest.model.model_id.as_str()),
    })
}

/// Lowercase, `[a-z0-9_-]`, 2..128 characters, never starting with `-`/`_`:
/// the catalog's identifier grammar, with a stable fallback.
fn catalog_token(raw: &str) -> String {
    let id: String = raw
        .to_ascii_lowercase()
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '_' | '-') {
                c
            } else {
                '-'
            }
        })
        .take(128)
        .collect();
    let id = id.trim_start_matches(['-', '_']).to_owned();
    if ds_command_kernel::grid_publication::catalog_identifier(&id) {
        id
    } else {
        "unnamed-source".into()
    }
}

fn migration(
    path: &str,
    backup: bool,
    raw_digest: &str,
    manifest_id: &str,
) -> Option<MigrationSource> {
    let reference = Path::new(path).file_name()?.to_str()?.to_owned();
    let id = catalog_token(manifest_id);
    Some(MigrationSource {
        kind: if backup {
            "pls_cadd_bak"
        } else {
            "dsgrid_package"
        }
        .into(),
        reference,
        source_digest: raw_digest.to_owned(),
        manifest_id: id,
    })
}

/// The package to publish, its conversion receipt when it came from a
/// backup, and the SHA-256 of the exact source bytes read from `path`.
fn incoming_bytes(
    inputs: &Inputs,
    path: &str,
) -> Result<(Vec<u8>, Option<Value>, String), Failure> {
    let extension = Path::new(path)
        .extension()
        .and_then(|x| x.to_str())
        .unwrap_or("");
    if extension.eq_ignore_ascii_case("dsgrid") {
        if inputs.value("crs").is_some()
            || inputs.value("select-project").is_some()
            || inputs.switch("swap-xy")
        {
            return Err(Failure::invalid(
                "replace_content_target_required",
                "PLS backup options cannot be used with .dsgrid",
            ));
        }
        let bytes = crate::package::read_bytes(path)?;
        let digest = format!("{:x}", Sha256::digest(&bytes));
        return Ok((bytes, None, digest));
    }
    if !extension.eq_ignore_ascii_case("bak") {
        return Err(Failure::invalid(
            "unsupported_model_source",
            "replacement source must be .dsgrid or .bak",
        ));
    }
    let crs = inputs.value("crs").ok_or_else(|| {
        Failure::invalid(
            "backup_crs_required",
            "PLS-CADD backup import needs a declared projected CRS",
        )
    })?;
    let bytes = crate::package::read_bytes(path)?;
    let raw_digest = format!("{:x}", Sha256::digest(&bytes));
    let name = Path::new(path)
        .file_name()
        .and_then(|x| x.to_str())
        .ok_or_else(|| {
            Failure::invalid(
                "unsupported_model_source",
                "PLS-CADD backup needs a UTF-8 filename",
            )
        })?;
    let sources = SourceSet::new(vec![SourceCandidate::file(name, bytes)]);
    let project = inputs
        .value("select-project")
        .map(|leaf| {
            resolve_pls_project_selection(&sources, leaf)
                .map_err(|e| Failure::invalid("backup_selection_invalid", format!("{leaf}: {e}")))
        })
        .transpose()?;
    let request = ConversionRequest {
        sources,
        batch_mode: BatchMode::Separate,
        target: TargetFormat::Dsgrid,
        pls_version_intent: PlsVersionIntent::ConvertTo16_81,
        pls_container: PlsContainer::Folder,
        combine: None,
        declared_crs: Some(crs.into()),
        expected_location: None,
        swap_xy: inputs.switch("swap-xy"),
        selection: Vec::new(),
        pls_project: project,
    };
    let plan = plan_conversion(&request);
    if !plan.blockers.is_empty() || !plan.losses.is_empty() {
        return Err(Failure::invalid(
            "backup_conversion_blocked",
            "backup conversion plan has blockers or losses",
        )
        .detail(
            json!({"blockers": plan.blockers, "warnings": plan.warnings, "losses": plan.losses}),
        ));
    }
    let result = execute_conversion(&plan, &request.sources)
        .map_err(|e| Failure::invalid("backup_conversion_failed", e.to_string()))?;
    if result.status != OutcomeStatus::Completed
        || result.outputs.len() != 1
        || !result.outputs[0].relative_path.ends_with(".dsgrid")
    {
        return Err(Failure::invalid(
            "backup_conversion_failed",
            "backup did not yield exactly one .dsgrid",
        )
        .detail(json!({"sources": result.per_source})));
    }
    let bytes = result.outputs[0].bytes.clone();
    let package_sha256 = format!("{:x}", Sha256::digest(&bytes));
    Ok((
        bytes,
        Some(json!({
            "input": path,
            "plan_id": plan.plan_id,
            "source_digests": plan.pinned_digests,
            "warnings": plan.warnings,
            "losses": plan.losses,
            "package_sha256": package_sha256,
            "per_source": result.per_source,
        })),
        raw_digest,
    ))
}

fn import_failure(error: ImportAsVersionError) -> Failure {
    use ds_grid_exchange::lineage_transfer::LineageTransferError::*;
    let message = error.to_string();
    match error {
        WorkingDigestMismatch | TargetDigestMismatch => {
            Failure::invalid("replace_content_invalid", message)
        }
        WorkingIdentityMismatch | TargetIdentityMismatch => {
            Failure::invalid("replace_content_invalid", message)
        }
        RevisionOverflow => Failure::invalid("replace_content_invalid", message),
        WorkingPackage(_) | TargetPackage(_) | ResultPackage(_) | ContentChanged => {
            Failure::invalid("replace_content_invalid", message)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> Vec<u8> {
        std::fs::read(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../../ds-network/fixtures/pls-public/humble-pole/humble-pole.dsgrid"
        ))
        .expect("the ds-network humble-pole fixture")
    }

    #[test]
    fn a_converted_package_names_the_exact_workspace_it_was_read_from() {
        let origin = workspace_origin(&fixture()).expect("the fixture records its workspace");
        assert_eq!(origin.kind, "pls_cadd_workspace");
        assert_eq!(origin.reference, "pls-original-workspace.bak");
        // The digest of the preserved original .bak member, as the package
        // attests it — the same bytes `dsgrid model link` digests against.
        assert_eq!(
            origin.source_digest,
            "d5be8a41b680f97148143b4323d3f43ce00cdf01193ba5fdd3555d9b741b7076"
        );
        assert!(ds_command_kernel::grid_publication::catalog_identifier(
            &origin.manifest_id
        ));
        assert!(workspace_origin(b"not a package").is_none());
    }

    #[test]
    fn provenance_tokens_follow_the_catalog_grammar() {
        assert_eq!(catalog_token("PLS_CADD workspace"), "pls_cadd-workspace");
        assert_eq!(catalog_token("--x"), "unnamed-source");
        assert_eq!(catalog_token(&"A".repeat(300)).len(), 128);
        let (path, purpose) = split_attach("/nonexistent/delivered.bak:native_workspace");
        assert_eq!(
            (path.as_str(), purpose.as_deref()),
            ("/nonexistent/delivered.bak", Some("native_workspace"))
        );
        // A path whose tail is not a plain token keeps its colon.
        let (path, purpose) = split_attach("/work/a:b/delivered.bak");
        assert_eq!((path.as_str(), purpose), ("/work/a:b/delivered.bak", None));
    }
}
