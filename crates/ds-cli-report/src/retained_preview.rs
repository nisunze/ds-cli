//! Retained project A4 PDFs: opening reads; explicit Refresh captures and publishes.
//! Rendering and publication remain the reporter and shared native owners.
use std::ffi::{OsStr, OsString};
use std::io::Read;
use std::path::Path;
use std::time::{Duration, Instant};

use ds_cli_auth::SavedA4Capture;
use ds_cli_contract::spec::{
    Arg, Authority, Chapter, Command, Effect, Example, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Failure, Inputs};
use ds_command_kernel::printing::{
    catalogue::Location,
    retained_preview::{self, Binding},
};
use ds_command_kernel::{compute_artifact_inventory::Head, report_export};
use ds_report_artifacts::{VerifiedSidecarArtifact, confined_fs::HeldDirectory};
use ds_sync_runtime::reports::{SealOutcome, SealRequest};
use serde_json::{Value, json};

const TRANSFORMER: Arg = Arg::value(
    "transformer",
    "<canonical-name>",
    "Exact saved transformer key, as returned by project scope.",
)
.required();
const TEMPLATE: Arg = Arg::value("template", "<id>", "Project-owned A4 document.")
    .choices(&[retained_preview::TEMPLATE])
    .default(retained_preview::TEMPLATE);
const OUT: Arg = Arg::value(
    "out",
    "<absolute-pdf>",
    "Fresh PDF file in an existing directory; existing files are refused.",
)
.required();
const STATE: Arg = Arg::value(
    "server-state-dir",
    "<absolute-path>",
    "Same native state root as ds server serve; omit for the lane default.",
);
const INPUTS_INVALID: Refusal = Refusal {
    code: "preview_inputs_invalid",
    when: "the explicit project, canonical transformer, template or saved-source pins are invalid",
    remedy: "use the exact project and transformer returned by project scope, with its current saved analysis and owned A4 document",
};
const OUTPUT_INVALID: Refusal = Refusal {
    code: "preview_output_invalid",
    when: "the PDF destination is not fresh, absolute or writable",
    remedy: "use a fresh absolute PDF filename in an existing directory",
};
const ENGINE_INVALID: Refusal = Refusal {
    code: "preview_engine_invalid",
    when: "the reporter is not a closed release build or lacks the governed A4 task",
    remedy: "install the release reporter advertising governed project A4 capture",
};
const RENDER_FAILED: Refusal = Refusal {
    code: "preview_render_failed",
    when: "the reporter refuses rendering or its PDF and receipt disagree",
    remedy: "inspect the actual reporter blockers; do not substitute engineering inputs or a template",
};
const PENDING: Refusal = Refusal {
    code: "preview_publication_pending",
    when: "the verified PDF is sealed but cloud publication or exact head readback has not succeeded",
    remedy: "preserve the named PDF and shared queue; use report outbox status and drain, then read the retained preview",
};
const REFUSALS: &[Refusal] = &[
    INPUTS_INVALID,
    OUTPUT_INVALID,
    ENGINE_INVALID,
    RENDER_FAILED,
    PENDING,
    Refusal {
        code: "publication_not_permitted",
        when: "the native user cannot read the project's retained PDF",
        remedy: "check this account's project membership",
    },
    Refusal {
        code: "publication_not_found",
        when: "no retained preview exists for this transformer and template",
        remedy: "explicitly refresh this preview once",
    },
    Refusal {
        code: "publication_conflict",
        when: "the retained head is inconsistent",
        remedy: "report the head identity; preserve retained artifacts",
    },
    Refusal {
        code: "publication_unavailable",
        when: "the publication service or native session cannot be reached",
        remedy: "retry the same retained read when the service is available",
    },
    Refusal {
        code: "publication_unreadable",
        when: "the retained head, ticket or PDF fails native validation",
        remedy: "preserve the head and retry the same read; do not recalculate or bypass validation",
    },
    crate::project::NATIVE_PROFILE,
    crate::project::NATIVE_PROFILE_DIGEST,
    crate::project::NATIVE_PROFILE_UNSAFE,
    crate::project::HEADLESS_SIGNED_OUT,
    crate::project::AUTH_CONTEXT_MISMATCH,
    crate::project::AUTH_REVOKED,
    crate::project::AUTH_TRANSIENT,
];

pub static READ: Command = Command {
    id: "report.preview.read",
    path: &["report", "preview", "read"],
    contract: 1,
    summary: "Download the retained project A4 preview PDF.",
    purpose: "Download the exact cloud-retained PDF for one project template and saved transformer. Opening reads only the existing head and verified PDF bytes: it does not capture current inputs, render, or wake the publication queue. Previously rendered PDFs remain readable when inputs change. The receipt shows captured provenance and states that current input freshness was not observed.",
    chapter: Chapter::Reports,
    effect: Effect::ArtifactWrite,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[
        crate::project::LANE_ARG,
        crate::project::PROJECT_ARG,
        TRANSFORMER,
        TEMPLATE,
        OUT,
    ],
    output: "Verified PDF path, SHA-256, size, actual head revision and captured source binding; current_inputs is not_observed.",
    examples: &[Example {
        command: "ds report preview read --project <exact-id> --transformer gashariki --out /absolute/new-preview.pdf --yes --output json",
        note: "Downloads the displayed revision without rendering again.",
        runnable: false,
    }],
    refusals: REFUSALS,
    reference: Some("docs/reference/report.md"),
    search: &["printing", "cached preview", "A4", "voltage drop", "PDF"],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};
pub static REFRESH: Command = Command {
    id: "report.preview.refresh",
    path: &["report", "preview", "refresh"],
    contract: 1,
    summary: "Render and publish a new retained project A4 preview PDF.",
    purpose: "Explicitly refresh one A4 preview from one native session's exact saved voltage-drop JSON, transformer layers, project config, governed A4 document and renderer style policy. The release reporter renders and verifies a PDF, then the shared publication queue uploads it and exact cloud readback proves delivery. Every explicit refresh has a fresh run identity, including unchanged inputs; earlier cloud artifacts remain retained. No analysis is recomputed and no template or style is invented.",
    chapter: Chapter::Reports,
    effect: Effect::ArtifactWrite,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[
        crate::project::LANE_ARG,
        crate::project::PROJECT_ARG,
        TRANSFORMER,
        TEMPLATE,
        OUT,
        STATE,
    ],
    output: "Verified PDF, new cloud head and binding, actual renderer receipt/build and shared reconciliation; failed publication explicitly retains the local PDF and sealed queue.",
    examples: &[Example {
        command: "ds report preview refresh --project <exact-id> --transformer gashariki --out /absolute/new-preview.pdf --yes --output json",
        note: "Creates a new immutable rendered revision and downloads those same bytes.",
        runnable: false,
    }],
    refusals: REFUSALS,
    reference: Some("docs/reference/report.md"),
    search: &["printing", "refresh preview", "A4", "voltage drop", "PDF"],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};

fn invalid(message: impl std::fmt::Display) -> Failure {
    Failure::invalid(INPUTS_INVALID.code, message.to_string()).remedy(INPUTS_INVALID.remedy)
}
fn output_invalid(message: impl std::fmt::Display) -> Failure {
    Failure::invalid(OUTPUT_INVALID.code, message.to_string()).remedy(OUTPUT_INVALID.remedy)
}
fn render_failed(message: impl std::fmt::Display) -> Failure {
    Failure::failed(RENDER_FAILED.code, message.to_string()).remedy(RENDER_FAILED.remedy)
}

pub fn preflight(inputs: &Inputs) -> Result<(), Failure> {
    let project = inputs.require("project")?;
    if project.is_empty()
        || project.len() > 128
        || !project.as_bytes()[0].is_ascii_alphanumeric()
        || !project
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
    {
        return Err(invalid("Use an exact project id."));
    }
    report_export::reportable_transformer(inputs.require("transformer")?).map_err(invalid)?;
    if inputs.require("template")? != retained_preview::TEMPLATE {
        return Err(invalid(
            "The preview template is not the owned A4 document.",
        ));
    }
    let path = Path::new(inputs.require("out")?);
    if !path.is_absolute()
        || path
            .extension()
            .is_none_or(|extension| !extension.eq_ignore_ascii_case("pdf"))
        || std::fs::symlink_metadata(path).is_ok()
        || !path.parent().is_some_and(Path::is_dir)
    {
        return Err(output_invalid(
            "Use a fresh absolute PDF path in an existing directory.",
        ));
    }
    if inputs
        .value("server-state-dir")
        .is_some_and(|state| !Path::new(state).is_absolute())
    {
        return Err(invalid("The server state directory must be absolute."));
    }
    Ok(())
}

fn stage(out: &Path) -> Result<tempfile::TempDir, Failure> {
    tempfile::Builder::new()
        .prefix(".ds-preview-")
        .tempdir_in(
            out.parent()
                .ok_or_else(|| output_invalid("No output parent."))?,
        )
        .map_err(output_invalid)
}

fn copy_fresh(source: &Path, out: &Path) -> Result<(), Failure> {
    let mut target = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(out)
        .map_err(output_invalid)?;
    let attempt = std::fs::File::open(source)
        .and_then(|mut file| std::io::copy(&mut file, &mut target))
        .and_then(|_| target.sync_all());
    if let Err(error) = attempt {
        drop(target);
        let _ = std::fs::remove_file(out);
        return Err(output_invalid(error));
    }
    Ok(())
}

fn answer(lane: &str, project: &str, transformer: &str, out: &Path, head: &Head) -> Value {
    json!({"schema":"ds.printing-preview.download/v1", "lane":lane, "project":project, "transformer":transformer, "template":retained_preview::TEMPLATE, "publication":"published", "pdf":{"path":out,"sha256":head.outputs[0].sha256,"bytes":head.outputs[0].size_bytes,"paper":"A4"}, "head":head,"current_inputs":"not_observed"})
}

pub fn read(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    preflight(inputs)?;
    let lane = inputs.require("lane")?;
    let project = inputs.require("project")?;
    let transformer = inputs.require("transformer")?;
    let out = Path::new(inputs.require("out")?);
    let stage = stage(out)?;
    let held_pdf = stage.path().join("retained.pdf");
    let head = ds_cli_auth::retained_a4_for_project(lane, project, transformer, &held_pdf)?;
    copy_fresh(&held_pdf, out)?;
    Ok(answer(lane, project, transformer, out, &head))
}

fn binding(capture: &SavedA4Capture, transformer: &str) -> Result<Binding, Failure> {
    let receipt = capture.input_receipt.as_ref().map_err(invalid)?;
    let version = i64::try_from(capture.version).map_err(invalid)?;
    let Location::GovernedHtml {
        revision_id,
        html_sha256,
        ..
    } = &capture.location
    else {
        return Err(invalid("The captured A4 is not an owned HTML document."));
    };
    let binding = Binding {
        schema: retained_preview::SCHEMA.into(),
        template_id: retained_preview::TEMPLATE.into(),
        template_revision_id: revision_id.clone(),
        template_html_sha256: html_sha256.clone(),
        analysis_sha256: capture.analysis_sha256.clone(),
        sheets_sha256: receipt.sheets_sha256.clone(),
        report_input_fingerprint: report_export::input_base_fingerprint(
            transformer,
            version,
            &receipt.sheets_sha256,
            &receipt.country,
            &receipt.reference_semantic_sha256,
        )
        .map_err(invalid)?,
        renderer_style_ref: capture.renderer_defaults.style_ref.clone(),
        renderer_revision_id: capture.renderer_defaults.revision_id.clone(),
        renderer_content_sha256: capture.renderer_defaults.content_sha256.clone(),
        transformer_revision: version,
        content_digest: capture.content_digest.clone(),
    };
    binding.validate().map_err(invalid)?;
    Ok(binding)
}

fn release_engine() -> Result<report_export::EngineIdentity, Failure> {
    let raw = crate::DS_REPORT.call_json("build-info", &[], crate::DISCOVERY_TIMEOUT)?;
    let engine = report_export::engine_identity(&raw).map_err(|error| {
        Failure::unavailable(ENGINE_INVALID.code, error).remedy(ENGINE_INVALID.remedy)
    })?;
    if engine.profile != "release"
        || engine.publication_state() != Ok(ds_command_kernel::report::PublicationState::Pending)
    {
        return Err(Failure::unavailable(
            ENGINE_INVALID.code,
            "Retained previews require an exact release renderer.",
        )
        .remedy(ENGINE_INVALID.remedy));
    }
    Ok(engine)
}

fn read_receipt(path: &Path) -> Result<Value, Failure> {
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .and_then(|file| file.take(1024 * 1024 + 1).read_to_end(&mut bytes))
        .map_err(render_failed)?;
    if bytes.len() > 1024 * 1024 {
        return Err(render_failed(
            "The renderer receipt exceeds its byte bound.",
        ));
    }
    serde_json::from_slice(&bytes).map_err(render_failed)
}

pub fn refresh(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    preflight(inputs)?;
    let lane = inputs.require("lane")?;
    let project = inputs.require("project")?;
    let transformer = inputs.require("transformer")?;
    let out = Path::new(inputs.require("out")?);
    let state = inputs.value("server-state-dir");
    crate::export::voltage_drop_project_a4_preflight()?;
    crate::export::voltage_drop_browser_preflight().map_err(render_failed)?;
    let engine = release_engine()?;
    let fence = ds_cli_auth::capture_layer_scope_fence_for_project(lane, project)?;
    let held = ds_cli_auth::saved_owned_a4_for_project(lane, project, transformer)?;
    let guard = || {
        ds_cli_auth::verify_layer_scope_fence_for_project(
            lane,
            &fence,
            held.identity().uid(),
            project,
        )
    };
    guard()?;
    let capture = held.result();
    let binding = binding(capture, transformer)?;
    let fingerprint = binding
        .input_base_fingerprint(project, transformer)
        .map_err(invalid)?;
    let room_digest = report_export::jcs::room_content_sha256(&capture.layers).map_err(invalid)?;
    let stage = stage(out)?;
    let source = stage.path().join("saved-analysis.json");
    ds_layer_store::private::write(&source, &capture.analysis).map_err(render_failed)?;
    let filename =
        ds_command_kernel::report_formats::report_filename(transformer, "voltage_drop_pdf")
            .map_err(invalid)?;
    let pdf = stage.path().join(&filename);
    let request = stage.path().join("request.json");
    let result = stage.path().join("renderer-receipt.json");
    let document = json!({"schema":"ds.voltage-drop-pdf.render-request/v1", "source_document":source,"source_sha256":capture.analysis_sha256,"layers":capture.layers,"network_config":capture.network_config,"renderer_defaults":capture.renderer_defaults,"report_format":capture.report_format,"transformer":transformer,"project_label":held.project_name(),"out_pdf":pdf});
    let bytes = serde_json::to_vec(&document).map_err(render_failed)?;
    if bytes.len() > 96 * 1024 * 1024 {
        return Err(render_failed(
            "The captured render request exceeds its byte bound.",
        ));
    }
    ds_layer_store::private::write(&request, bytes).map_err(render_failed)?;
    guard()?;
    let completed = crate::DS_REPORT.call(
        "render-voltage-drop-result",
        &[
            OsString::from("--request"),
            request.into_os_string(),
            OsString::from("--result"),
            result.clone().into_os_string(),
        ],
        crate::EXPORT_TIMEOUT,
    )?;
    let receipt = read_receipt(&result)?;
    if !completed.succeeded() {
        return Err(
            render_failed("The actual renderer refused this captured preview.")
                .detail(json!({"renderer_receipt":receipt,"exit_status":completed.status})),
        );
    }
    crate::export::verify_voltage_drop_pdf(
        &receipt,
        &pdf,
        &capture.analysis_sha256,
        transformer,
        held.project_name(),
    )
    .map_err(render_failed)?;
    if release_engine()? != engine {
        return Err(Failure::unavailable(
            ENGINE_INVALID.code,
            "The renderer build changed during rendering.",
        )
        .remedy(ENGINE_INVALID.remedy));
    }
    guard()?;
    copy_fresh(&pdf, out)?;
    let artifacts = [VerifiedSidecarArtifact {
        format: "voltage_drop_pdf".into(),
        filename: filename.clone(),
        size_bytes: receipt["bytes"]
            .as_u64()
            .ok_or_else(|| render_failed("No verified PDF byte count."))?,
        sha256: receipt["output_sha256"]
            .as_str()
            .ok_or_else(|| render_failed("No verified PDF digest."))?
            .into(),
        paper_size: Some("A4".into()),
        presentation: None,
        held_file: HeldDirectory::open_absolute(stage.path())
            .map_err(render_failed)?
            .ok_or_else(|| render_failed("The captured directory disappeared."))?
            .open_regular_file(OsStr::new(&filename), "verified preview PDF")
            .map_err(render_failed)?,
    }];
    let queue = crate::project::export::PublicationQueue::open(lane, state.map(Path::new))?;
    let run_id = ds_report_host::random_run_id();
    let request = SealRequest {
        owner_uid: held.identity().uid(),
        project_id: project,
        engine_version: &engine.engine_version,
        engine_build_manifest_sha256: &engine.build_manifest_sha256,
        transformer,
        transformer_revision: binding.transformer_revision,
        input_base_fingerprint: &fingerprint,
        room_content_sha256: &room_digest,
        client_run_id: &run_id,
        artifacts: &artifacts,
        expected_formats: &["voltage_drop_pdf"],
        deadline: Instant::now() + Duration::from_secs(30),
        guard: &|| guard().map_err(|failure| failure.to_string()),
    };
    let sealed = ds_sync_runtime::reports::seal_printing_preview(&queue.store, &queue.fence, &queue.root, &request, &binding).map_err(|error| Failure::failed(PENDING.code, error).remedy(PENDING.remedy).detail(json!({"pdf":out,"sha256":artifacts[0].sha256,"run_id":run_id,"publication":"not_proven"})))?;
    let (seal, replay_key) = match sealed {
        SealOutcome::Sealed(sealed) => (json!(sealed.receipt), sealed.receipt.client_publish_id),
        SealOutcome::AlreadyRecorded { replay_key, state } => {
            (json!({"replay_key":replay_key,"state":state}), replay_key)
        }
    };
    guard()?;
    let reconciliation = crate::touch::project(lane, project, state);
    let acknowledged_work = ds_sync_runtime::reports::acknowledged_printing_preview_work(
        &queue.store, &queue.fence, project, transformer, &replay_key,
    ).map_err(|error| Failure::failed(PENDING.code, error).remedy(PENDING.remedy).detail(json!({"pdf":out,"sha256":artifacts[0].sha256,"run_id":run_id,"seal":seal,"reconciliation":reconciliation.receipt,"publication":"not_proven"})))?;
    let cloud_pdf = stage.path().join("cloud-retained.pdf");
    let head = ds_cli_auth::retained_a4_for_project(lane, project, transformer, &cloud_pdf).map_err(|failure| Failure::failed(PENDING.code, "The rendered PDF is sealed; exact cloud readback has not succeeded.").remedy(PENDING.remedy).detail(json!({"pdf":out,"sha256":artifacts[0].sha256,"run_id":run_id,"seal":seal,"reconciliation":reconciliation.receipt,"readback_refusal":failure.code(),"publication":"not_proven"})))?;
    if head.work_id != acknowledged_work
        || head.engine_version.as_deref() != Some(engine.engine_version.as_str())
        || head.engine_build_manifest_sha256.as_deref()
            != Some(engine.build_manifest_sha256.as_str())
        || head.printing_preview.as_ref() != Some(&binding)
        || head.outputs[0].sha256 != artifacts[0].sha256
        || head.outputs[0].size_bytes != artifacts[0].size_bytes
    {
        return Err(Failure::failed(PENDING.code, "The cloud head differs from this explicitly refreshed PDF.").remedy(PENDING.remedy).detail(json!({"pdf":out,"sha256":artifacts[0].sha256,"run_id":run_id,"seal":seal,"head":head,"publication":"not_proven"})));
    }
    guard()?;
    let mut value = answer(lane, project, transformer, out, &head);
    value["run_id"] = json!(run_id);
    value["renderer"] = json!(engine);
    value["renderer_receipt"] = receipt;
    value["seal"] = seal;
    value["reconciliation"] = reconciliation.receipt;
    Ok(value)
}

pub fn render(value: &Value) -> String {
    format!("{value}\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    fn inputs(command: &'static Command, out: &Path, transformer: &str) -> Inputs {
        ds_cli_contract::args::parse(
            command,
            &[
                "--project".into(),
                "project_a".into(),
                "--transformer".into(),
                transformer.into(),
                "--out".into(),
                out.display().to_string(),
            ],
        )
        .unwrap()
    }
    #[test]
    fn invalid_selectors_and_existing_outputs_refuse_before_session_or_renderer() {
        let root = tempfile::tempdir_in(std::env::current_dir().unwrap()).unwrap();
        let out = root.path().join("preview.pdf");
        assert_eq!(
            preflight(&inputs(&READ, &out, "UPPERCASE"))
                .unwrap_err()
                .code(),
            INPUTS_INVALID.code
        );
        std::fs::write(&out, b"held").unwrap();
        assert_eq!(
            preflight(&inputs(&REFRESH, &out, "transformer_a"))
                .unwrap_err()
                .code(),
            OUTPUT_INVALID.code
        );
        assert_eq!(std::fs::read(&out).unwrap(), b"held");
    }
    #[test]
    fn copying_the_download_never_overwrites_a_destination_created_after_preflight() {
        let root = tempfile::tempdir_in(std::env::current_dir().unwrap()).unwrap();
        let source = root.path().join("source.pdf");
        let out = root.path().join("preview.pdf");
        std::fs::write(&source, b"new").unwrap();
        std::fs::write(&out, b"held").unwrap();
        assert_eq!(
            copy_fresh(&source, &out).unwrap_err().code(),
            OUTPUT_INVALID.code
        );
        assert_eq!(std::fs::read(out).unwrap(), b"held");
    }
}
