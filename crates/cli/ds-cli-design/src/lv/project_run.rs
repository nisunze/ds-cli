//! Compose the existing fenced export, native process and verified save paths.
//! This adapter authors receipts, never model inputs or analysis documents.
use std::path::{Path, PathBuf};

use ds_cli_contract::spec::{
    Arg, Authority, Chapter, Command, Effect, Example, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Failure, Handler, Inputs, parse, success_envelope};
use serde_json::{Value, json};

use super::{
    artifact::{ArtifactContract, sha256, write_new},
    process, project_export, project_save,
};

const LOCAL_REFUSALS: &[Refusal] = &[
    Refusal {
        code: "fast_lv_run_output_exists",
        when: "--out-dir exists without --resume",
        remedy: "choose an absent directory or resume the same captured run with --resume",
    },
    Refusal {
        code: "fast_lv_run_write_failed",
        when: "the run directory, lease or receipt cannot be durably created",
        remedy: "retain the captured artifacts; repair directory access before resuming",
    },
    Refusal {
        code: "fast_lv_run_incomplete",
        when: "a captured stage is incomplete, mismatched, oversized or modified",
        remedy: "keep the evidence and use a new output directory",
    },
    Refusal {
        code: "fast_lv_run_busy",
        when: "another invocation holds this run directory's lease",
        remedy: "wait for that invocation; after a crash remove only the abandoned .run-lock file",
    },
    Refusal {
        code: "fast_lv_run_process_failed",
        when: "the single native job did not succeed",
        remedy: "inspect result.json diagnostics and fix the project source before a new run",
    },
    Refusal {
        code: "fast_lv_run_save_unverified",
        when: "the save did not verify exactly one transformer's version, layer and analysis digests",
        remedy: "inspect detail.save; resume the captured run to retry its save",
    },
    Refusal {
        code: "fast_lv_run_print_failed",
        when: "the save succeeded but the analysis read or A4 print failed",
        remedy: "keep detail.save and the output directory, then retry --resume --print-a4",
    },
];

const fn save_declares(code: &str) -> bool {
    let mut index = 0;
    while index < project_save::COMMAND.refusals.len() {
        let other = project_save::COMMAND.refusals[index].code.as_bytes();
        let bytes = code.as_bytes();
        let mut byte = 0;
        if bytes.len() == other.len() {
            while byte < bytes.len() && bytes[byte] == other[byte] {
                byte += 1;
            }
            if byte == bytes.len() {
                return true;
            }
        }
        index += 1;
    }
    false
}

const fn process_extra_count() -> usize {
    let mut count = 0;
    let mut index = 0;
    while index < process::COMMAND.refusals.len() {
        if !save_declares(process::COMMAND.refusals[index].code) {
            count += 1;
        }
        index += 1;
    }
    count
}

pub(super) const BROWSER_MISSING: Refusal = Refusal {
    code: "reporter_browser_missing",
    when: "no verified existing reporter browser is configured",
    remedy: "run ds workstation configure --component chromium --target reporter --yes once",
};
const PRINT_REFUSALS: &[Refusal] = &[
    BROWSER_MISSING,
    Refusal {
        code: "reporter_pdf_unverified",
        when: "the PDF disagrees with its source, digest, bytes or page receipt",
        remedy: "retain the attempt and retry --resume --print-a4",
    },
];
const fn report_extra_count() -> usize {
    let mut count = 0;
    let mut i = 0;
    while i < ds_cli_report::export::COMMAND.refusals.len() {
        if !save_declares(ds_cli_report::export::COMMAND.refusals[i].code) {
            count += 1;
        }
        i += 1;
    }
    count + PRINT_REFUSALS.len()
}

const fn refusals() -> [Refusal;
    LOCAL_REFUSALS.len()
        + project_save::COMMAND.refusals.len()
        + process_extra_count()
        + report_extra_count()] {
    let mut result = [LOCAL_REFUSALS[0];
        LOCAL_REFUSALS.len()
            + project_save::COMMAND.refusals.len()
            + process_extra_count()
            + report_extra_count()];
    let mut offset = 0;
    let lists = [LOCAL_REFUSALS, project_save::COMMAND.refusals];
    let mut list = 0;
    while list < lists.len() {
        let mut index = 0;
        while index < lists[list].len() {
            result[offset] = lists[list][index];
            offset += 1;
            index += 1;
        }
        list += 1;
    }
    let mut index = 0;
    while index < process::COMMAND.refusals.len() {
        let refusal = process::COMMAND.refusals[index];
        if !save_declares(refusal.code) {
            result[offset] = refusal;
            offset += 1;
        }
        index += 1;
    }
    let mut index = 0;
    while index < ds_cli_report::export::COMMAND.refusals.len() {
        let refusal = ds_cli_report::export::COMMAND.refusals[index];
        if !save_declares(refusal.code) {
            result[offset] = refusal;
            offset += 1;
        }
        index += 1;
    }
    let mut index = 0;
    while index < PRINT_REFUSALS.len() {
        result[offset] = PRINT_REFUSALS[index];
        offset += 1;
        index += 1;
    }
    result
}

pub static COMMAND: Command = Command {
    id: "design.lv.project-run",
    path: &["design", "lv", "project-run"],
    contract: 2,
    summary: "Compute, save and optionally print one LV transformer to A4.",
    purpose: "Capture the project's fenced transformer and configuration through project-export --project-config, process once with ds-network owner defaults or --preset/--operator, then project-save the exact layers and ds.lv-voltage-drop.analysis/v1 atomically on the working head. Success requires the save owner's verified layer/analysis digests. Retain the bounded output directory; --resume reuses its captured source/result and deterministic operation ID, never re-exporting or recomputing. Named versions and backups are unchanged; nothing is uploaded. With --print-a4, capture the lane's managed Reporter once, preflight its render_voltage_drop_result task and headless browser before effects, then fetch the exact saved analysis under all three save pins and call report.export voltage-drop with matching held layers and the sealed project caption. A missing or mismatched caption refuses printing. Print failures retain saved:true/printed:false evidence. --resume --print-a4 retries from the verified save without saving again; moved heads refuse. Configure a browser once with workstation configure --component chromium --target reporter. No window is opened.",
    chapter: Chapter::Design,
    effect: Effect::GlobalWrite,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[
        Arg::value(
            "project",
            "<id>",
            "Exact project to authorize for every read and write; never reads saved selection.",
        )
        .required(),
        Arg::value(
            "transformer",
            "<name>",
            "One exact transformer in that project.",
        )
        .required(),
        Arg::value(
            "out-dir",
            "<path>",
            "Absent directory for request, result and bounded export/process/save receipts.",
        )
        .required(),
        ds_cli_contract::spec::Arg {
            summary: "Native credential lane for the entire captured run.",
            ..ds_cli_contract::spec::LANE
        },
        Arg::switch(
            "resume",
            "Reuse captured export/process bytes; with --print-a4 retry the first successful save's print without saving again.",
        ),
        Arg::switch(
            "print-a4",
            "Render verified saved analysis to local A4 PDF with the configured headless browser.",
        ),
        project_export::PRESET,
        project_export::OPERATOR,
    ],
    output: "Explicit project/lane/transformer, deterministic operation_id, input/result digests, captured artifact paths, and the exact verified save receipt. saved is true after owner verification; printed is true only after PDF receipt/digest/page verification. Print errors carry saved:true/printed:false and exact save evidence in detail. Generated PDFs stay local in the run directory.",
    examples: &[
        Example {
            command: "ds design lv project-run --project <id> --transformer T-1042 --out-dir ./T-1042-run --yes --output json",
            note: "Capture, process once and verify the atomic working-head save.",
            runnable: false,
        },
        Example {
            command: "ds design lv project-run --project <id> --transformer T-1042 --out-dir ./T-1042-run --print-a4 --yes --output json",
            note: "Compute, save and print with an existing configured browser.",
            runnable: false,
        },
        Example {
            command: "ds design lv project-run --project <id> --transformer T-1042 --out-dir ./T-1042-run --resume --print-a4 --yes --output json",
            note: "Retry printing the first verified save without exporting, processing or saving again.",
            runnable: false,
        },
    ],
    refusals: &refusals(),
    reference: Some("docs/reference/design.md"),
    search: &["one shot", "voltage drop", "compute save"],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};

const RECEIPT: ArtifactContract = ArtifactContract {
    name: "LV run receipt",
    stage_tag: "ds-lv-run",
    exists_code: "fast_lv_run_output_exists",
    write_code: "fast_lv_run_write_failed",
    exists_remedy: "Retain the captured run and choose an absent receipt path.",
    write_remedy: "Retain the captured artifacts and repair directory access before resuming.",
};

fn incomplete(message: impl Into<String>) -> Failure {
    Failure::invalid("fast_lv_run_incomplete", message)
        .remedy("Retain the evidence and use a new output directory; never assemble or repair process JSON manually.")
}

fn read_receipt(path: &Path, command: &Command) -> Result<Value, Failure> {
    let bytes = project_save::read(&path.to_string_lossy(), 1024 * 1024)
        .map_err(|error| incomplete(error.message()))?;
    project_save::receipt(&bytes, command.id).map_err(|error| incomplete(error.message()))
}

fn keep_receipt(path: &Path, command: &Command, value: Value) -> Result<Value, Failure> {
    let bytes = serde_json::to_vec(&success_envelope(
        command.id,
        command.contract,
        value.clone(),
    ))
    .map_err(|error| incomplete(error.to_string()))?;
    if bytes.len() > 1024 * 1024 {
        return Err(incomplete("Run receipt exceeds 1 MiB."));
    }
    write_new(path, &bytes, &RECEIPT)?;
    Ok(value)
}

// Only this closed set of existing semantic handlers is callable. No shell,
// sibling argv, alternate transport or model authoring is added here.
type SavedAnalysisRead = fn(&str, &str, &str, u64, &str, &str) -> Result<Vec<u8>, Failure>;

struct Owners {
    export: Handler,
    process: Handler,
    save: fn(&Inputs, &Context, &str) -> Result<Value, Failure>,
    preflight: fn(&str) -> Result<Option<ds_cli_exec::CapturedExternal>, Failure>,
    analysis: SavedAnalysisRead,
    renderer_policy: fn(&str, &str) -> Result<Value, Failure>,
    print: fn(&Inputs, &Context, Option<&ds_cli_exec::CapturedExternal>) -> Result<Value, Failure>,
}
const OWNERS: Owners = Owners {
    export: project_export::run,
    process: process::run_receipt,
    save: project_save::run_for_project,
    preflight: print_preflight,
    analysis: ds_cli_auth::transformer_analysis_for_project,
    renderer_policy,
    print: print_captured,
};

fn renderer_policy(lane: &str, project: &str) -> Result<Value, Failure> {
    let snapshot: ds_command_kernel::style_resolution::Snapshot =
        serde_json::from_value(ds_cli_auth::style_governance(
            lane,
            project,
            &ds_command_kernel::style_governance::Command::Table,
        )?)
        .map_err(|error| incomplete(error.to_string()))?;
    if snapshot.project_id != project {
        return Err(incomplete("Renderer policy belongs to another project."));
    }
    let binding =
        ds_command_kernel::printing::renderer_defaults::resolve(&snapshot).map_err(incomplete)?;
    serde_json::to_value(binding).map_err(|error| incomplete(error.to_string()))
}

fn require_owned_a4(config: &Value, project: &str) -> Result<(), Failure> {
    if config["printing_context"]["project_id"] != project {
        return Err(incomplete(
            "A4 capture lacks the exact run project context.",
        ));
    }
    let document = &config["printing_a4"];
    if document["schema"] != "ds.print-a4-document/v1" || document["project_id"] != project {
        return Err(incomplete(
            "A4 capture lacks the named project's governed document.",
        ));
    }
    Ok(())
}

pub(super) fn print_preflight(
    lane: &str,
) -> Result<Option<ds_cli_exec::CapturedExternal>, Failure> {
    let engine = ds_cli_report::DS_REPORT
        .capture_for_lane_in(lane, &ds_cli_exec::addons::managed_module_base()?)?;
    ds_cli_report::export::voltage_drop_project_a4_preflight_captured(&engine)?;
    ds_cli_report::export::voltage_drop_browser_preflight().map_err(|message| {
        Failure::unavailable("reporter_browser_missing", message).remedy(PRINT_REFUSALS[0].remedy)
    })?;
    Ok(Some(engine))
}

fn print_captured(
    inputs: &Inputs,
    _: &Context,
    engine: Option<&ds_cli_exec::CapturedExternal>,
) -> Result<Value, Failure> {
    let engine =
        engine.ok_or_else(|| incomplete("A4 print requires its preflight Reporter capture."))?;
    ds_cli_report::export::run_captured(inputs, engine)
}

fn pdf_unverified(message: String) -> Failure {
    Failure::failed("reporter_pdf_unverified", message).remedy(PRINT_REFUSALS[1].remedy)
}

fn invoke(
    command: &Command,
    handler: impl FnOnce(&Inputs, &Context) -> Result<Value, Failure>,
    args: &[(&str, &str)],
    switches: &[&str],
    context: &Context,
) -> Result<Value, Failure> {
    let mut tokens = Vec::new();
    for (key, value) in args {
        tokens.extend([format!("--{key}"), (*value).to_owned()]);
    }
    tokens.extend(switches.iter().map(|key| format!("--{key}")));
    handler(&parse(command, &tokens)?, context)
}

struct Lease(PathBuf);
impl Drop for Lease {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

pub fn run(inputs: &Inputs, context: &Context) -> Result<Value, Failure> {
    run_with(inputs, context, &OWNERS)
}

fn run_with(inputs: &Inputs, context: &Context, owners: &Owners) -> Result<Value, Failure> {
    if !context.confirmed {
        return Err(
            Failure::invalid("confirmation_required", "Project run requires --yes.")
                .remedy("Review the explicit project and transformer, then pass --yes."),
        );
    }
    let lane = inputs.require("lane")?;
    let reporter = if inputs.switch("print-a4") {
        match (owners.preflight)(lane) {
            Ok(engine) => engine,
            Err(error) => {
                // A failed preflight has no effects. On a print retry preserve the
                // validated captured save evidence even if the reporter vanished.
                return Err(match captured_save_for_preflight(inputs) {
                    Ok(Some(saved)) => error.detail(json!({"saved":true,"printed":false,"save":saved,"out_dir":inputs.require("out-dir")?})),
                    _ => error,
                });
            }
        }
    } else {
        None
    };
    let project = inputs.require("project")?;
    let transformer = inputs.require("transformer")?;
    let directory = PathBuf::from(inputs.require("out-dir")?);
    if inputs.switch("resume") {
        if !directory
            .symlink_metadata()
            .is_ok_and(|m| m.is_dir() && !m.file_type().is_symlink())
        {
            return Err(incomplete("Resume requires a real captured run directory."));
        }
    } else {
        std::fs::create_dir(&directory).map_err(|error| {
            if error.kind() == std::io::ErrorKind::AlreadyExists {
                Failure::conflict(
                    "fast_lv_run_output_exists",
                    "The run directory already exists.",
                )
                .remedy(LOCAL_REFUSALS[0].remedy)
            } else {
                Failure::failed("fast_lv_run_write_failed", error.to_string())
                    .remedy(RECEIPT.write_remedy)
            }
        })?;
    }
    let directory = directory
        .canonicalize()
        .map_err(|error| incomplete(error.to_string()))?;
    let lock = directory.join(".run-lock");
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&lock)
        .map_err(|error| {
            if error.kind() == std::io::ErrorKind::AlreadyExists {
                Failure::conflict("fast_lv_run_busy", "This run directory is leased.")
                    .remedy(LOCAL_REFUSALS[3].remedy)
            } else {
                Failure::failed("fast_lv_run_write_failed", error.to_string())
                    .remedy(RECEIPT.write_remedy)
            }
        })?;
    let _lease = Lease(lock);
    let request = directory.join("request.json");
    let result = directory.join("result.json");
    let source_path = directory.join("source.json");
    let process_path = directory.join("process.json");
    let save_path = directory.join("save.json");
    let request_text = request.to_string_lossy();
    let result_text = result.to_string_lossy();
    let source = if inputs.switch("resume") {
        read_receipt(&source_path, &project_export::COMMAND)?
    } else {
        let mut export_args = vec![
            ("project", project),
            ("transformer", transformer),
            ("lane", lane),
            ("out", request_text.as_ref()),
        ];
        for key in ["preset", "operator"] {
            if let Some(value) = inputs.value(key) {
                export_args.push((key, value));
            }
        }
        let source = invoke(
            &project_export::COMMAND,
            owners.export,
            &export_args,
            &["project-config"],
            context,
        )?;
        keep_receipt(&source_path, &project_export::COMMAND, source)?
    };
    if source["project"]["ds_project"] != project
        || source["transformer"] != transformer
        || source["lane"] != lane
        || source["project_config"] != "included"
    {
        return Err(incomplete(
            "Captured project, transformer, lane or configuration does not match this run.",
        ));
    }
    let input_bytes = project_save::read(
        &request_text,
        ds_network_contracts::lv_processing::MAX_NATIVE_LV_INPUT_BYTES,
    )?;
    if source["request_sha256"] != sha256(&input_bytes) {
        return Err(incomplete(
            "Captured request does not match its export receipt.",
        ));
    }
    let processed = if inputs.switch("resume") {
        read_receipt(&process_path, &process::COMMAND)?
    } else {
        let processed = invoke(
            &process::COMMAND,
            owners.process,
            &[("input", &request_text), ("out", &result_text)],
            &[],
            context,
        )?;
        keep_receipt(&process_path, &process::COMMAND, processed)?
    };
    if processed["jobs"] != 1 || processed["succeeded"] != 1 || processed["failed"] != 0 {
        return Err(Failure::failed(
            "fast_lv_run_process_failed",
            "The single native job did not succeed; save was not attempted.",
        )
        .remedy(LOCAL_REFUSALS[4].remedy)
        .detail(json!({"process": processed, "result_path": result})));
    }
    let result_bytes = project_save::read(
        &result_text,
        ds_network_contracts::lv_processing::MAX_NATIVE_LV_OUTPUT_BYTES,
    )?;
    let (captured_project, _, _) = project_save::verify_receipts(
        &source,
        &processed,
        transformer,
        lane,
        &input_bytes,
        &result_bytes,
    )?;
    if captured_project != project {
        return Err(incomplete("Captured source addresses another project."));
    }
    let operation_id = operation_id(&source, &processed);
    let saved = if inputs.switch("resume")
        && inputs.switch("print-a4")
        && save_path
            .try_exists()
            .map_err(|error| incomplete(error.to_string()))?
    {
        let bytes = project_save::read(&save_path.to_string_lossy(), 1024 * 1024)?;
        let envelope: Value =
            serde_json::from_slice(&bytes).map_err(|error| incomplete(error.to_string()))?;
        let saved = project_save::receipt(&bytes, project_save::COMMAND.id)?;
        if !envelope["run_operation_id"].is_null() && envelope["run_operation_id"] != operation_id {
            return Err(incomplete(
                "First save receipt does not match this captured operation.",
            ));
        }
        saved
    } else {
        invoke(
            &project_save::COMMAND,
            |inputs, context| (owners.save)(inputs, context, project),
            &[
                ("source", &source_path.to_string_lossy()),
                ("input", &request_text),
                ("result", &result_text),
                ("process-receipt", &process_path.to_string_lossy()),
                ("transformer", transformer),
                ("operation-id", &operation_id),
                ("lane", lane),
            ],
            &[],
            context,
        )?
    };
    require_verified_save(&saved, project, transformer, lane)?;
    // Save-only retries refresh owner verification. Print retries use the first
    // immutable save receipt and verify its fence through the raw analysis read.
    if !save_path
        .try_exists()
        .map_err(|error| incomplete(error.to_string()))?
    {
        let mut envelope = serde_json::to_value(success_envelope(
            project_save::COMMAND.id,
            project_save::COMMAND.contract,
            saved.clone(),
        ))
        .map_err(|error| incomplete(error.to_string()))?;
        envelope["run_operation_id"] = json!(operation_id);
        let bytes = serde_json::to_vec(&envelope).map_err(|error| incomplete(error.to_string()))?;
        if bytes.len() > 1024 * 1024 {
            return Err(incomplete("Save receipt exceeds 1 MiB."));
        }
        write_new(&save_path, &bytes, &RECEIPT)?;
    }
    let mut outcome = json!({"project":project, "lane":lane, "transformer":transformer,
        "operation_id":operation_id, "input_sha256":processed["input_sha256"],
        "result_sha256":processed["result_sha256"], "out_dir":directory,
        "artifacts":{"source":source_path,"input":request,"result":result,"process_receipt":process_path,"first_verified_save_receipt":save_path},
        "saved":true,"printed":false,"save":saved});
    if inputs.switch("print-a4") {
        match print_saved(owners, context, reporter.as_ref(), PrintRun { directory: &directory, project, transformer, lane, saved: &saved, source: &source, input: &input_bytes, result: &result_bytes }) {
            Ok(printed) => { outcome["printed"] = json!(true); outcome["print"] = printed; }
            Err(error) => return Err(Failure::failed("fast_lv_run_print_failed", "The transformer was saved and verified; A4 printing failed.")
                .remedy(LOCAL_REFUSALS[6].remedy)
                .detail(json!({"saved":true,"printed":false,"save":saved,"run":outcome,"print_error":json!({"code":error.code(),"message":error.message(),"remedy":error.remedy_text(),"detail":error.detail_value()})}))),
        }
    }
    Ok(outcome)
}

fn captured_save_for_preflight(inputs: &Inputs) -> Result<Option<Value>, Failure> {
    if !inputs.switch("resume") {
        return Ok(None);
    }
    let directory = Path::new(inputs.require("out-dir")?);
    let project = inputs.require("project")?;
    let transformer = inputs.require("transformer")?;
    let lane = inputs.require("lane")?;
    let source = read_receipt(&directory.join("source.json"), &project_export::COMMAND)?;
    let processed = read_receipt(&directory.join("process.json"), &process::COMMAND)?;
    let input = project_save::read(
        &directory.join("request.json").to_string_lossy(),
        ds_network_contracts::lv_processing::MAX_NATIVE_LV_INPUT_BYTES,
    )?;
    let result = project_save::read(
        &directory.join("result.json").to_string_lossy(),
        ds_network_contracts::lv_processing::MAX_NATIVE_LV_OUTPUT_BYTES,
    )?;
    let (captured_project, _, _) =
        project_save::verify_receipts(&source, &processed, transformer, lane, &input, &result)?;
    if captured_project != project {
        return Err(incomplete("Captured save addresses another project."));
    }
    let bytes = project_save::read(&directory.join("save.json").to_string_lossy(), 1024 * 1024)?;
    let envelope: Value =
        serde_json::from_slice(&bytes).map_err(|error| incomplete(error.to_string()))?;
    if !envelope["run_operation_id"].is_null()
        && envelope["run_operation_id"] != operation_id(&source, &processed)
    {
        return Err(incomplete("Save operation differs."));
    }
    let saved = project_save::receipt(&bytes, project_save::COMMAND.id)?;
    require_verified_save(&saved, project, transformer, lane)?;
    Ok(Some(saved))
}

struct PrintRun<'a> {
    directory: &'a Path,
    project: &'a str,
    transformer: &'a str,
    lane: &'a str,
    saved: &'a Value,
    source: &'a Value,
    input: &'a [u8],
    result: &'a [u8],
}

fn print_saved(
    owners: &Owners,
    context: &Context,
    reporter: Option<&ds_cli_exec::CapturedExternal>,
    run: PrintRun<'_>,
) -> Result<Value, Failure> {
    let input: Value =
        serde_json::from_slice(run.input).map_err(|error| incomplete(error.to_string()))?;
    let network_config = &input["jobs"][0]["config_dfs"];
    require_owned_a4(network_config, run.project)?;
    let capture = &run.source["project"]["printing_receipt"];
    let receipt_path = run.directory.join("request.printing-receipt.json");
    if capture["path"]
        .as_str()
        .is_none_or(|p| Path::new(p) != receipt_path.as_path())
        || !receipt_path
            .symlink_metadata()
            .is_ok_and(|m| m.is_file() && !m.file_type().is_symlink())
    {
        return Err(incomplete(
            "The captured project label has no owned receipt artifact.",
        ));
    }
    let receipt_bytes = project_save::read(
        &receipt_path.to_string_lossy(),
        ds_network_contracts::lv_processing::MAX_NATIVE_LV_INPUT_BYTES,
    )?;
    if capture["sha256"] != sha256(&receipt_bytes) || capture["bytes"] != receipt_bytes.len() {
        return Err(incomplete("The captured printing receipt bytes moved."));
    }
    let printing_receipt: ds_command_kernel::report_export::InputReceipt =
        serde_json::from_slice(&receipt_bytes).map_err(|e| incomplete(e.to_string()))?;
    let project_label = ds_command_kernel::report_export::captured_printing_project_label(
        run.project,
        run.source,
        network_config,
        &printing_receipt,
    )
    .map_err(incomplete)?;
    let row = &run.saved["result"]["results"][0];
    let content_digest = row["content_digest"]
        .as_str()
        .expect("verified save digest");
    let source_sha = row["analysis_sha256"]
        .as_str()
        .expect("verified analysis digest");
    // Fresh fenced server read on EVERY retry; never treat local analysis as
    // evidence that the working head still holds the captured save.
    let analysis = (owners.analysis)(
        run.lane,
        run.project,
        run.transformer,
        row["version"].as_u64().expect("verified version"),
        content_digest,
        source_sha,
    )?;
    if analysis.len() > ds_client_core::TRANSFORMER_ANALYSIS_RESPONSE_LIMIT
        || sha256(&analysis) != source_sha
    {
        return Err(incomplete(
            "Saved analysis bytes do not match the verified save.",
        ));
    }
    let result: Value =
        serde_json::from_slice(run.result).map_err(|error| incomplete(error.to_string()))?;
    let jobs = result["jobs"]
        .as_array()
        .ok_or_else(|| incomplete("No captured result jobs."))?;
    if jobs.len() != 1 || jobs[0]["transformer_name"] != run.transformer || jobs[0]["ok"] != true {
        return Err(incomplete(
            "Captured layers do not belong to this successful transformer.",
        ));
    }
    let layers: std::collections::BTreeMap<String, Value> =
        serde_json::from_value(jobs[0]["output"]["gdfs"].clone())
            .map_err(|error| incomplete(error.to_string()))?;
    if ds_command_kernel::report_export::jcs::layers_content_digest(&layers).map_err(incomplete)?
        != content_digest
    {
        return Err(incomplete(
            "Held process layers do not match the exact saved layer digest.",
        ));
    }
    let format =
        ds_command_kernel::report_formats::project_voltage_drop_report_format(network_config)
            .map_err(incomplete)?
            .token();
    let receipt_path = run.directory.join("print.json");
    let mut previous_receipt = None;
    if receipt_path
        .try_exists()
        .map_err(|error| incomplete(error.to_string()))?
    {
        let receipt = read_receipt(&receipt_path, &ds_cli_report::export::COMMAND)?;
        let pdf = receipt["out_pdf"]
            .as_str()
            .ok_or_else(|| incomplete("Print receipt has no PDF."))?;
        let pdf = Path::new(pdf);
        if !pdf.starts_with(run.directory)
            || pdf
                .file_name()
                .is_none_or(|name| name != "voltage-drop-a4.pdf")
        {
            return Err(incomplete("Print receipt points outside the captured run."));
        }
        if receipt["project_label"] == run.project {
            // A genuine earlier print may carry the old ID-caption defect.
            // Verify every old artifact before selecting a fresh print attempt;
            // preserve the original owner's report and PDF, never patch either.
            ds_cli_report::export::verify_voltage_drop_pdf(
                &receipt,
                pdf,
                source_sha,
                run.transformer,
                run.project,
            )
            .map_err(pdf_unverified)?;
            let old_report = read_receipt(
                &pdf.parent()
                    .ok_or_else(|| incomplete("Legacy PDF has no attempt directory."))?
                    .join("report.json"),
                &ds_cli_report::export::COMMAND,
            )?;
            if old_report != receipt
                || !receipt_path
                    .symlink_metadata()
                    .is_ok_and(|m| m.is_file() && !m.file_type().is_symlink())
            {
                return Err(incomplete(
                    "The old ID-caption receipt has no matching preserved native attempt.",
                ));
            }
            previous_receipt = Some(project_save::read(
                &receipt_path.to_string_lossy(),
                1024 * 1024,
            )?);
        } else {
            ds_cli_report::export::verify_voltage_drop_pdf(
                &receipt,
                pdf,
                source_sha,
                run.transformer,
                &project_label,
            )
            .map_err(pdf_unverified)?;
            return Ok(receipt);
        }
    }
    // Every attempt gets absent paths. Preserve all previous attempts, even
    // when an owner wrote a PDF but failed before returning its receipt.
    let attempt = (1..=1000)
        .find_map(|number| {
            let path = run.directory.join(format!("print-{number}"));
            match std::fs::create_dir(&path) {
                Ok(()) => Some(Ok(path)),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => None,
                Err(error) => Some(Err(incomplete(error.to_string()))),
            }
        })
        .ok_or_else(|| incomplete("Print attempt limit reached; retain the run evidence."))??;
    let analysis_path = attempt.join("saved-analysis.json");
    write_new(&analysis_path, &analysis, &RECEIPT)?;
    let request_path = attempt.join("request.json");
    let pdf = attempt.join("voltage-drop-a4.pdf");
    let renderer_defaults = (owners.renderer_policy)(run.lane, run.project)?;
    let request = json!({"schema":"ds.voltage-drop-pdf.render-request/v1",
        "network_config":network_config,"renderer_defaults":renderer_defaults,
        "source_document":analysis_path,"source_sha256":source_sha,"layers":layers,
        "report_format":format,"transformer":run.transformer,"project_label":project_label,"out_pdf":pdf});
    write_new(
        &request_path,
        &serde_json::to_vec(&request).map_err(|error| incomplete(error.to_string()))?,
        &RECEIPT,
    )?;
    let printed = invoke(
        &ds_cli_report::export::COMMAND,
        |inputs, context| (owners.print)(inputs, context, reporter),
        &[
            ("lane", run.lane),
            ("task", "voltage-drop"),
            ("request", &request_path.to_string_lossy()),
        ],
        &[],
        context,
    )?;
    // Retain returned owner evidence even when verification fails.
    keep_receipt(
        &attempt.join("report.json"),
        &ds_cli_report::export::COMMAND,
        printed.clone(),
    )?;
    ds_cli_report::export::verify_voltage_drop_pdf(
        &printed,
        &pdf,
        source_sha,
        run.transformer,
        &project_label,
    )
    .map_err(pdf_unverified)?;
    if let Some(previous) = previous_receipt {
        if !receipt_path
            .symlink_metadata()
            .is_ok_and(|m| m.is_file() && !m.file_type().is_symlink())
            || project_save::read(&receipt_path.to_string_lossy(), 1024 * 1024)? != previous
        {
            return Err(incomplete(
                "The selected print receipt moved during correction; attempts are preserved.",
            ));
        }
        write_new(&attempt.join("previous-print.json"), &previous, &RECEIPT)?;
        let selected = attempt.join("selected-print.json");
        keep_receipt(&selected, &ds_cli_report::export::COMMAND, printed.clone())?;
        std::fs::rename(&selected, &receipt_path).map_err(|e| incomplete(e.to_string()))?;
        if let Ok(directory) = std::fs::File::open(run.directory) {
            let _ = directory.sync_all();
        }
        Ok(printed)
    } else {
        keep_receipt(&receipt_path, &ds_cli_report::export::COMMAND, printed)
    }
}

fn operation_id(source: &Value, processed: &Value) -> String {
    // Only stable identity/fences/digests; paths and timings cannot change a retry.
    let coordinates = json!([
        "ds.lv.project-run/v1",
        source["project"]["ds_project"],
        source["lane"],
        source["transformer"],
        source["source"]["version"],
        source["source"]["content_digest"],
        processed["input_sha256"],
        processed["result_sha256"]
    ]);
    format!("lv-run-{}", sha256(coordinates.to_string().as_bytes()))
}

fn require_verified_save(
    saved: &Value,
    project: &str,
    transformer: &str,
    lane: &str,
) -> Result<(), Failure> {
    let rows = saved["result"]["results"].as_array();
    let verified = saved["project"] == project
        && saved["result"]["project_id"] == project
        && saved["lane"] == lane
        && rows.is_some_and(|rows| {
            rows.len() == 1
                && rows[0]["transformer_name"] == transformer
                && rows[0]["verified"] == true
                && rows[0]["error_code"].is_null()
                && rows[0]["version"]
                    .as_u64()
                    .zip(rows[0]["content_digest"].as_str())
                    .zip(rows[0]["analysis_sha256"].as_str())
                    .is_some_and(|((version, digest), sha)| {
                        ds_client_core::validate_saved_lv_analysis_pins(version, digest, sha)
                            .is_ok()
                    })
        });
    if !verified {
        return Err(Failure::failed(
            "fast_lv_run_save_unverified",
            "The save owner did not verify this transformer; no saved result or print is claimed.",
        )
        .remedy(LOCAL_REFUSALS[5].remedy)
        .detail(json!({"save":saved})));
    }
    Ok(())
}

pub fn render(value: &Value) -> String {
    format!(
        "LV transformer {} saved and verified in {}.\nCaptured run: {}\nOperation: {}\nPrinted: {}",
        value["transformer"].as_str().unwrap_or(""),
        value["project"].as_str().unwrap_or(""),
        value["out_dir"].as_str().unwrap_or(""),
        value["operation_id"].as_str().unwrap_or(""),
        value["printed"]
    )
}

#[cfg(test)]
#[path = "project_run_tests.rs"]
mod tests;
