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
        remedy: "retain the evidence and use a new output directory; never assemble or repair process JSON manually",
    },
    Refusal {
        code: "fast_lv_run_busy",
        when: "another invocation holds this run directory's lease",
        remedy: "wait for that invocation; after a crash remove only the abandoned .run-lock file before resuming",
    },
    Refusal {
        code: "fast_lv_run_process_failed",
        when: "the single native job did not succeed",
        remedy: "inspect result.json diagnostics and correct the project source before starting a new run",
    },
    Refusal {
        code: "fast_lv_run_save_unverified",
        when: "the save owner did not verify exactly one transformer with version, layer and analysis digests",
        remedy: "inspect detail.save; resume the unchanged captured run to retry its fenced save",
    },
    Refusal {
        code: "fast_lv_run_print_failed",
        when: "the verified save succeeded but the fenced analysis read or A4 print failed",
        remedy: "retain detail.save and the output directory, then retry --resume --print-a4; never reconstruct saved analysis",
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

const PRINT_REFUSALS: &[Refusal] = &[
    Refusal {
        code: "reporter_browser_missing",
        when: "no verified existing reporter browser is configured",
        remedy: "run ds workstation configure --component chromium --target reporter --yes --output json once",
    },
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
    contract: 1,
    summary: "Compute, save and optionally print one LV transformer to A4.",
    purpose: "Capture the explicit project's currently fenced transformer and configuration through project-export --project-config, process once with ds-network owner defaults, then project-save the exact layers and ds.lv-voltage-drop.analysis/v1 atomically on the working head. Success requires the save owner's fresh verified layer/analysis digests. Retain the bounded output directory; --resume reuses its exact captured source/result and deterministic operation ID, never re-exports or recomputes. Named versions and backups are not changed; no compute artifacts are uploaded. With --print-a4, preflight the installed render_voltage_drop_result task and configured headless browser before effects, then fetch exact saved analysis under all three save pins and call report.export voltage-drop with matching held layers. Print failures retain saved:true/printed:false evidence. --resume --print-a4 retries from the first verified save without saving again; moved heads refuse. Configure an existing browser once with workstation configure --component chromium --target reporter. No Desktop or browser window is opened.",
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
        Arg::value(
            "lane",
            "<stable|canary>",
            "Native credential lane for the entire captured run.",
        )
        .default("stable")
        .choices(&["stable", "canary"]),
        Arg::switch(
            "resume",
            "Reuse captured export/process bytes; with --print-a4 retry the first successful save's print without saving again.",
        ),
        Arg::switch(
            "print-a4",
            "Render verified saved analysis to local A4 PDF with the configured headless browser.",
        ),
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
    search: &["one shot", "voltage drop", "compute save", "a4"],
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
    preflight: fn() -> Result<(), Failure>,
    analysis: SavedAnalysisRead,
    print: Handler,
}
const OWNERS: Owners = Owners {
    export: project_export::run,
    process: process::run,
    save: project_save::run_for_project,
    preflight: print_preflight,
    analysis: ds_cli_auth::transformer_analysis_for_project,
    print: ds_cli_report::export::run,
};

fn print_preflight() -> Result<(), Failure> {
    ds_cli_report::export::voltage_drop_preflight()?;
    ds_cli_report::export::voltage_drop_browser_preflight().map_err(|message| {
        Failure::unavailable("reporter_browser_missing", message).remedy(PRINT_REFUSALS[0].remedy)
    })
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
    if inputs.switch("print-a4")
        && let Err(error) = (owners.preflight)()
    {
        // A failed preflight has no effects. On a print retry preserve the
        // validated captured save evidence even if the reporter vanished.
        return Err(match captured_save_for_preflight(inputs) {
            Ok(Some(saved)) => error.detail(json!({"saved":true,"printed":false,"save":saved,"out_dir":inputs.require("out-dir")?})),
            _ => error,
        });
    }
    let project = inputs.require("project")?;
    let transformer = inputs.require("transformer")?;
    let lane = inputs.require("lane")?;
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
        let source = invoke(
            &project_export::COMMAND,
            owners.export,
            &[
                ("project", project),
                ("transformer", transformer),
                ("lane", lane),
                ("out", &request_text),
            ],
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
        ds_network::network::native_fast_lv::MAX_NATIVE_FAST_LV_INPUT_BYTES,
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
        ds_network::network::native_fast_lv::MAX_NATIVE_FAST_LV_OUTPUT_BYTES,
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
        match print_saved(owners, context, PrintRun { directory: &directory, project, transformer, lane, saved: &saved, input: &input_bytes, result: &result_bytes }) {
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
        ds_network::network::native_fast_lv::MAX_NATIVE_FAST_LV_INPUT_BYTES,
    )?;
    let result = project_save::read(
        &directory.join("result.json").to_string_lossy(),
        ds_network::network::native_fast_lv::MAX_NATIVE_FAST_LV_OUTPUT_BYTES,
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
    input: &'a [u8],
    result: &'a [u8],
}

fn print_saved(owners: &Owners, context: &Context, run: PrintRun<'_>) -> Result<Value, Failure> {
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
    let input: Value =
        serde_json::from_slice(run.input).map_err(|error| incomplete(error.to_string()))?;
    let format = ds_command_kernel::report_formats::project_voltage_drop_report_format(
        &input["jobs"][0]["config_dfs"],
    )
    .map_err(incomplete)?
    .token();
    let receipt_path = run.directory.join("print.json");
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
        ds_cli_report::export::verify_voltage_drop_pdf(
            &receipt,
            pdf,
            source_sha,
            run.transformer,
            run.project,
        )
        .map_err(pdf_unverified)?;
        return Ok(receipt);
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
    let request = json!({"schema":"ds.voltage-drop-pdf.render-request/v1",
        "source_document":analysis_path,"source_sha256":source_sha,"layers":layers,
        "report_format":format,"transformer":run.transformer,"project_label":run.project,"out_pdf":pdf});
    write_new(
        &request_path,
        &serde_json::to_vec(&request).map_err(|error| incomplete(error.to_string()))?,
        &RECEIPT,
    )?;
    let printed = invoke(
        &ds_cli_report::export::COMMAND,
        owners.print,
        &[
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
        run.project,
    )
    .map_err(pdf_unverified)?;
    keep_receipt(&receipt_path, &ds_cli_report::export::COMMAND, printed)
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
                && rows[0]["version"].as_u64().is_some_and(|v| v > 0)
                && ["content_digest", "analysis_sha256"].iter().all(|key| {
                    rows[0][key].as_str().is_some_and(|s| {
                        s.len() == 64
                            && s.bytes()
                                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
                    })
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
mod tests {
    use super::*;
    use ds_cli_contract::{Format, Output};
    use std::cell::RefCell;

    #[derive(Default)]
    struct Calls {
        stages: Vec<&'static str>,
        operations: Vec<String>,
        fail: Option<&'static str>,
        unverified: bool,
        brief: bool,
    }
    thread_local! { static CALLS: RefCell<Calls> = RefCell::default(); }

    fn stage(name: &'static str) -> Result<(), Failure> {
        CALLS.with(|calls| {
            let mut calls = calls.borrow_mut();
            calls.stages.push(name);
            if calls.fail == Some(name) {
                Err(Failure::failed("auth_transient", "Injected owner failure.")
                    .remedy("Retry the same captured operation."))
            } else {
                Ok(())
            }
        })
    }

    fn export(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
        stage("export")?;
        assert!(inputs.switch("project-config"));
        let bytes = ds_network::network::native_fast_lv::encode_native_fast_lv_request_with_config(
            inputs.require("transformer")?,
            &std::collections::BTreeMap::from([(
                "tr".to_owned(),
                json!({"type":"FeatureCollection","features":[]}),
            )]),
            &CALLS.with(|c| if c.borrow().brief {
                std::collections::BTreeMap::from([("project_settings".to_owned(), json!([{"parameter":"design_export_format","value":{"schema":"ds.design-output-selection/v1","voltage_drop_report":"brief"}}]))])
            } else { std::collections::BTreeMap::new() }),
        )
        .unwrap();
        write_new(
            Path::new(inputs.require("out")?),
            &bytes,
            &super::super::artifact::PROJECT_REQUEST,
        )?;
        Ok(json!({"project":{"ds_project":inputs.require("project")?},
            "transformer":inputs.require("transformer")?,"lane":inputs.require("lane")?,
            "source":{"state":"fenced","version":7,"content_digest":"a".repeat(64)},
            "project_config":"included","request_sha256":sha256(&bytes)}))
    }

    fn compute(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
        stage("process")?;
        let input = std::fs::read(inputs.require("input")?).unwrap();
        // Opaque owner bytes: composition must not rewrite or decode them.
        let output = serde_json::to_vec(&json!({"jobs":[{"transformer_name":"T1","ok":true,
            "output":{"gdfs":{"tr":{"type":"FeatureCollection","features":[]}}}}]}))
        .unwrap();
        write_new(
            Path::new(inputs.require("out")?),
            &output,
            &super::super::artifact::RESULT,
        )?;
        Ok(
            json!({"jobs":1,"succeeded":1,"failed":0,"input_sha256":sha256(&input),"result_sha256":sha256(&output)}),
        )
    }

    fn save(inputs: &Inputs, _: &Context, expected_project: &str) -> Result<Value, Failure> {
        CALLS.with(|calls| {
            calls
                .borrow_mut()
                .operations
                .push(inputs.require("operation-id").unwrap().to_owned())
        });
        stage("save")?;
        let source = project_save::receipt(
            &std::fs::read(inputs.require("source")?).unwrap(),
            project_export::COMMAND.id,
        )
        .unwrap();
        let processed = project_save::receipt(
            &std::fs::read(inputs.require("process-receipt")?).unwrap(),
            process::COMMAND.id,
        )
        .unwrap();
        let input = std::fs::read(inputs.require("input")?).unwrap();
        let output = std::fs::read(inputs.require("result")?).unwrap();
        let (project, version, _) = project_save::verify_receipts(
            &source,
            &processed,
            inputs.require("transformer")?,
            inputs.require("lane")?,
            &input,
            &output,
        )?;
        assert_eq!(project, expected_project);
        assert!(serde_json::from_slice::<Value>(&output).unwrap()["jobs"][0]["ok"] == true);
        let verified = CALLS.with(|calls| !calls.borrow().unverified);
        Ok(
            json!({"project":project,"lane":inputs.require("lane")?,"result":{"project_id":project,
            "results":[{"transformer_name":inputs.require("transformer")?,"verified":verified,
                "unchanged":false,"version":version+1,"content_digest":layer_digest(),"analysis_sha256":sha256(ANALYSIS)}]}}),
        )
    }

    const ANALYSIS: &[u8] = b"{ \"number\": 1.000, \"saved\": true }\n";
    fn layer_digest() -> String {
        ds_command_kernel::report_export::jcs::layers_content_digest(
            &std::collections::BTreeMap::from([(
                "tr".into(),
                json!({"type":"FeatureCollection","features":[]}),
            )]),
        )
        .unwrap()
    }
    fn preflight() -> Result<(), Failure> {
        stage("preflight")
    }
    fn analysis(
        lane: &str,
        project: &str,
        transformer: &str,
        version: u64,
        digest: &str,
        sha: &str,
    ) -> Result<Vec<u8>, Failure> {
        stage("analysis")?;
        assert_eq!(
            (lane, project, transformer, version),
            ("canary", "explicit-project", "T1", 8)
        );
        assert_eq!(digest, layer_digest());
        assert_eq!(sha, sha256(ANALYSIS));
        Ok(ANALYSIS.to_vec())
    }
    fn print(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
        stage("print")?;
        assert_eq!(inputs.require("task")?, "voltage-drop");
        let request: Value =
            serde_json::from_slice(&std::fs::read(inputs.require("request")?).unwrap()).unwrap();
        assert_eq!(
            std::fs::read(request["source_document"].as_str().unwrap()).unwrap(),
            ANALYSIS
        );
        assert_eq!(request["source_sha256"], sha256(ANALYSIS));
        assert_eq!(
            request["report_format"],
            CALLS.with(|c| if c.borrow().brief {
                "brief"
            } else {
                "extended"
            })
        );
        assert_eq!(request["project_label"], "explicit-project");
        let path = request["out_pdf"].as_str().unwrap();
        assert!(!Path::new(path).exists());
        let mut doc = lopdf::Document::with_version("1.5");
        let pages = doc.new_object_id();
        let page = doc.add_object(lopdf::dictionary! {"Type" => "Page", "Parent" => pages,
        "MediaBox" => vec![0.into(),0.into(),595.into(),842.into()]});
        doc.objects.insert(
            pages,
            lopdf::dictionary! {"Type" => "Pages", "Kids" => vec![page.into()], "Count" => 1}
                .into(),
        );
        let catalog = doc.add_object(lopdf::dictionary! {"Type" => "Catalog", "Pages" => pages});
        doc.trailer.set("Root", catalog);
        let mut pdf = Vec::new();
        doc.save_to(&mut pdf).unwrap();
        std::fs::write(path, &pdf).unwrap();
        Ok(
            json!({"schema":"ds.voltage-drop-pdf.render/v1","transformer":"T1","project_label":"explicit-project",
            "out_pdf":path,"source_sha256":sha256(ANALYSIS),"output_sha256":sha256(&pdf),
            "pages":1,"bytes":pdf.len(),"publication":"nothing_published"}),
        )
    }

    const MOCK: Owners = Owners {
        export,
        process: compute,
        save,
        preflight,
        analysis,
        print,
    };

    fn context(confirmed: bool) -> Context {
        Context {
            confirmed,
            output: Output::resolve(Format::Json, false, true),
        }
    }

    fn inputs(directory: &Path, flags: &[&str]) -> Inputs {
        let mut tokens = vec![
            "--project".into(),
            "explicit-project".into(),
            "--transformer".into(),
            "T1".into(),
            "--lane".into(),
            "canary".into(),
            "--out-dir".into(),
            directory.to_str().unwrap().into(),
        ];
        tokens.extend(flags.iter().map(|s| s.to_string()));
        parse(&COMMAND, &tokens).unwrap()
    }

    fn reset() {
        CALLS.with(|calls| *calls.borrow_mut() = Calls::default());
    }

    #[test]
    fn legacy_first_save_receipt_can_print_without_rewrite_or_resave() {
        reset();
        let temp = tempfile::tempdir().unwrap();
        let directory = temp.path().join("run");
        run_with(&inputs(&directory, &[]), &context(true), &MOCK).unwrap();
        let path = directory.join("save.json");
        let mut envelope: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        envelope.as_object_mut().unwrap().remove("run_operation_id");
        let first = serde_json::to_vec(&envelope).unwrap();
        std::fs::write(&path, &first).unwrap();
        let printed = run_with(
            &inputs(&directory, &["--resume", "--print-a4"]),
            &context(true),
            &MOCK,
        )
        .unwrap();
        assert_eq!(printed["printed"], true);
        assert_eq!(std::fs::read(&path).unwrap(), first);
        assert_eq!(
            CALLS.with(|c| c.borrow().stages.clone()),
            [
                "export",
                "process",
                "save",
                "preflight",
                "analysis",
                "print"
            ]
        );
    }

    #[test]
    fn source_brief_selection_is_passed_explicitly_to_saved_analysis_presenter() {
        reset();
        CALLS.with(|c| c.borrow_mut().brief = true);
        let temp = tempfile::tempdir().unwrap();
        let directory = temp.path().join("run");
        let printed =
            run_with(&inputs(&directory, &["--print-a4"]), &context(true), &MOCK).unwrap();
        assert_eq!(printed["printed"], true);
    }

    #[test]
    fn retry_preflight_keeps_captured_successful_save_evidence_without_effects() {
        reset();
        let temp = tempfile::tempdir().unwrap();
        let directory = temp.path().join("run");
        run_with(&inputs(&directory, &[]), &context(true), &MOCK).unwrap();
        fn missing() -> Result<(), Failure> {
            Err(Failure::unavailable("reporter_browser_missing", "gone")
                .remedy("configure chromium for reporter"))
        }
        let owners = Owners {
            preflight: missing,
            ..MOCK
        };
        let failure = run_with(
            &inputs(&directory, &["--resume", "--print-a4"]),
            &context(true),
            &owners,
        )
        .unwrap_err();
        assert_eq!(failure.code(), "reporter_browser_missing");
        assert_eq!(failure.detail_value().unwrap()["saved"], true);
        assert_eq!(failure.detail_value().unwrap()["printed"], false);
        assert_eq!(
            CALLS.with(|c| c.borrow().stages.clone()),
            ["export", "process", "save"]
        );
    }

    #[test]
    fn print_preflight_refuses_before_directory_or_project_effects() {
        for code in [
            "reporter_engine_missing",
            "unknown_task",
            "reporter_browser_missing",
        ] {
            reset();
            let temp = tempfile::tempdir().unwrap();
            let directory = temp.path().join("run");
            fn missing_reporter() -> Result<(), Failure> {
                Err(Failure::unavailable("reporter_engine_missing", "missing")
                    .remedy("install reporter"))
            }
            fn missing_task() -> Result<(), Failure> {
                Err(Failure::unavailable("unknown_task", "missing").remedy("update reporter"))
            }
            fn missing_browser() -> Result<(), Failure> {
                Err(Failure::unavailable("reporter_browser_missing", "missing")
                    .remedy("configure chromium for reporter"))
            }
            let owners = Owners {
                preflight: match code {
                    "reporter_engine_missing" => missing_reporter,
                    "unknown_task" => missing_task,
                    _ => missing_browser,
                },
                ..MOCK
            };
            assert_eq!(
                run_with(
                    &inputs(&directory, &["--print-a4"]),
                    &context(true),
                    &owners
                )
                .unwrap_err()
                .code(),
                code
            );
            assert!(!directory.exists());
            assert!(CALLS.with(|c| c.borrow().stages.is_empty()));
        }
    }

    #[test]
    fn saved_but_print_failed_resumes_without_save_export_compute_or_overwrite() {
        reset();
        CALLS.with(|c| c.borrow_mut().fail = Some("print"));
        let temp = tempfile::tempdir().unwrap();
        let directory = temp.path().join("run");
        let error =
            run_with(&inputs(&directory, &["--print-a4"]), &context(true), &MOCK).unwrap_err();
        assert_eq!(error.code(), "fast_lv_run_print_failed");
        assert_eq!(error.detail_value().unwrap()["saved"], true);
        assert_eq!(error.detail_value().unwrap()["printed"], false);
        let first = std::fs::read(directory.join("save.json")).unwrap();
        assert_eq!(
            error.detail_value().unwrap()["save"],
            read_receipt(&directory.join("save.json"), &project_save::COMMAND).unwrap()
        );
        assert_eq!(
            std::fs::read(directory.join("print-1/saved-analysis.json")).unwrap(),
            ANALYSIS
        );
        // Simulate an owner that wrote partial output before failing. Retry
        // must choose a fresh attempt, never remove the failed output.
        std::fs::write(
            directory.join("print-1/voltage-drop-a4.pdf"),
            b"partial pdf",
        )
        .unwrap();
        CALLS.with(|c| c.borrow_mut().fail = None);
        let result = run_with(
            &inputs(&directory, &["--resume", "--print-a4"]),
            &context(true),
            &MOCK,
        )
        .unwrap();
        assert_eq!(result["saved"], true);
        assert_eq!(result["printed"], true);
        assert_eq!(std::fs::read(directory.join("save.json")).unwrap(), first);
        assert_eq!(
            std::fs::read(directory.join("print-1/voltage-drop-a4.pdf")).unwrap(),
            b"partial pdf"
        );
        let pdf = std::fs::read(directory.join("print-2/voltage-drop-a4.pdf")).unwrap();
        let receipt = std::fs::read(directory.join("print.json")).unwrap();
        let resumed = run_with(
            &inputs(&directory, &["--resume", "--print-a4"]),
            &context(true),
            &MOCK,
        )
        .unwrap();
        assert_eq!(resumed["print"], result["print"]);
        assert_eq!(
            std::fs::read(directory.join("print.json")).unwrap(),
            receipt
        );
        assert_eq!(
            std::fs::read(directory.join("print-2/voltage-drop-a4.pdf")).unwrap(),
            pdf
        );
        assert_eq!(
            CALLS.with(|c| c.borrow().stages.clone()),
            [
                "preflight",
                "export",
                "process",
                "save",
                "analysis",
                "print",
                "preflight",
                "analysis",
                "print",
                "preflight",
                "analysis"
            ]
        );
    }

    #[test]
    fn moved_head_refuses_print_from_first_save_without_recompute_or_resave() {
        fn moved(
            _: &str,
            project: &str,
            transformer: &str,
            version: u64,
            _: &str,
            _: &str,
        ) -> Result<Vec<u8>, Failure> {
            stage("analysis")?;
            assert_eq!(
                (project, transformer, version),
                ("explicit-project", "T1", 8)
            );
            Err(Failure::conflict("auth_response_unreadable", "moved head")
                .remedy("review working head")
                .detail(json!({"http_status":409,"service_code":"analysis_stale"})))
        }
        reset();
        let temp = tempfile::tempdir().unwrap();
        let directory = temp.path().join("run");
        run_with(&inputs(&directory, &[]), &context(true), &MOCK).unwrap();
        let first = std::fs::read(directory.join("save.json")).unwrap();
        let owners = Owners {
            analysis: moved,
            ..MOCK
        };
        let failure = run_with(
            &inputs(&directory, &["--resume", "--print-a4"]),
            &context(true),
            &owners,
        )
        .unwrap_err();
        assert_eq!(failure.detail_value().unwrap()["saved"], true);
        assert_eq!(
            failure.detail_value().unwrap()["print_error"]["detail"]["http_status"],
            409
        );
        assert_eq!(std::fs::read(directory.join("save.json")).unwrap(), first);
        assert_eq!(
            CALLS.with(|c| c.borrow().stages.clone()),
            ["export", "process", "save", "preflight", "analysis"]
        );
        assert!(!directory.join("print-1").exists());
    }

    #[test]
    fn mismatched_saved_layers_and_wrong_raw_analysis_never_reach_reporter() {
        fn wrong(_: &str, _: &str, _: &str, _: u64, _: &str, _: &str) -> Result<Vec<u8>, Failure> {
            stage("analysis")?;
            Ok(b"{}".to_vec())
        }
        for wrong_bytes in [true, false] {
            reset();
            let temp = tempfile::tempdir().unwrap();
            let directory = temp.path().join("run");
            run_with(&inputs(&directory, &[]), &context(true), &MOCK).unwrap();
            if !wrong_bytes {
                let path = directory.join("save.json");
                let mut saved: Value =
                    serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
                saved["data"]["result"]["results"][0]["content_digest"] = json!("a".repeat(64));
                std::fs::write(path, saved.to_string()).unwrap();
            }
            fn same(
                _: &str,
                _: &str,
                _: &str,
                _: u64,
                _: &str,
                _: &str,
            ) -> Result<Vec<u8>, Failure> {
                stage("analysis")?;
                Ok(ANALYSIS.to_vec())
            }
            let owners = Owners {
                analysis: if wrong_bytes { wrong } else { same },
                ..MOCK
            };
            assert_eq!(
                run_with(
                    &inputs(&directory, &["--resume", "--print-a4"]),
                    &context(true),
                    &owners
                )
                .unwrap_err()
                .code(),
                "fast_lv_run_print_failed"
            );
            assert!(!CALLS.with(|c| c.borrow().stages.contains(&"print")));
        }
    }

    #[test]
    fn composes_exact_owner_artifacts_and_retries_without_export_or_compute() {
        reset();
        let temp = tempfile::tempdir().unwrap();
        let directory = temp.path().join("run");
        let result = run_with(&inputs(&directory, &[]), &context(true), &MOCK).unwrap();
        assert_eq!(result["saved"], true);
        assert_eq!(result["printed"], false);
        assert_eq!(
            result["save"]["result"]["results"][0]["analysis_sha256"],
            sha256(ANALYSIS)
        );
        assert_eq!(
            CALLS.with(|c| c.borrow().stages.clone()),
            ["export", "process", "save"]
        );
        let first_save = std::fs::read(directory.join("save.json")).unwrap();
        let resumed = run_with(&inputs(&directory, &["--resume"]), &context(true), &MOCK).unwrap();
        assert_eq!(resumed["operation_id"], result["operation_id"]);
        assert_eq!(
            CALLS.with(|c| c.borrow().stages.clone()),
            ["export", "process", "save", "save"]
        );
        assert_eq!(
            std::fs::read(directory.join("save.json")).unwrap(),
            first_save
        );
        assert!(!directory.join(".run-lock").exists());
    }

    #[test]
    fn failed_stages_never_reach_later_owners_or_create_a_saved_receipt() {
        for (failure, expected) in [
            ("export", vec!["export"]),
            ("process", vec!["export", "process"]),
            ("save", vec!["export", "process", "save"]),
        ] {
            reset();
            CALLS.with(|calls| calls.borrow_mut().fail = Some(failure));
            let temp = tempfile::tempdir().unwrap();
            let directory = temp.path().join("run");
            assert_eq!(
                run_with(&inputs(&directory, &[]), &context(true), &MOCK)
                    .unwrap_err()
                    .code(),
                "auth_transient"
            );
            assert_eq!(CALLS.with(|c| c.borrow().stages.clone()), expected);
            assert!(!directory.join("save.json").exists());
            if failure == "save" {
                CALLS.with(|calls| calls.borrow_mut().fail = None);
                let result =
                    run_with(&inputs(&directory, &["--resume"]), &context(true), &MOCK).unwrap();
                assert_eq!(result["saved"], true);
                CALLS.with(|calls| {
                    let calls = calls.borrow();
                    assert_eq!(calls.operations.len(), 2);
                    assert_eq!(calls.operations[0], calls.operations[1]);
                    assert_eq!(calls.stages, ["export", "process", "save", "save"]);
                });
            }
        }
    }

    #[test]
    fn unverified_save_and_failed_native_job_are_failures() {
        reset();
        CALLS.with(|calls| calls.borrow_mut().unverified = true);
        let temp = tempfile::tempdir().unwrap();
        let directory = temp.path().join("unverified");
        assert_eq!(
            run_with(&inputs(&directory, &[]), &context(true), &MOCK)
                .unwrap_err()
                .code(),
            "fast_lv_run_save_unverified"
        );
        assert!(!directory.join("save.json").exists());
        reset();
        let directory = temp.path().join("native-failure");
        // Real owner request encoder and processor: empty source is not a
        // successful job, even though process writes a complete batch result.
        let native = Owners {
            export,
            process: process::run,
            save,
            ..MOCK
        };
        assert_eq!(
            run_with(&inputs(&directory, &[]), &context(true), &native)
                .unwrap_err()
                .code(),
            "fast_lv_run_process_failed"
        );
        assert_eq!(CALLS.with(|c| c.borrow().stages.clone()), ["export"]);
        assert!(directory.join("result.json").exists());
        assert!(directory.join("process.json").exists());
        assert!(!directory.join("save.json").exists());
    }

    #[test]
    fn print_confirmation_existing_directory_and_lease_refuse_before_owners() {
        reset();
        let temp = tempfile::tempdir().unwrap();
        let directory = temp.path().join("run");
        assert_eq!(
            run_with(&inputs(&directory, &[]), &context(false), &MOCK)
                .unwrap_err()
                .code(),
            "confirmation_required"
        );
        assert!(!directory.exists());
        std::fs::create_dir(&directory).unwrap();
        assert_eq!(
            run_with(&inputs(&directory, &[]), &context(true), &MOCK)
                .unwrap_err()
                .code(),
            "fast_lv_run_output_exists"
        );
        std::fs::write(directory.join(".run-lock"), b"another invocation").unwrap();
        assert_eq!(
            run_with(&inputs(&directory, &["--resume"]), &context(true), &MOCK)
                .unwrap_err()
                .code(),
            "fast_lv_run_busy"
        );
        assert_eq!(
            std::fs::read(directory.join(".run-lock")).unwrap(),
            b"another invocation"
        );
        assert!(CALLS.with(|c| c.borrow().stages.is_empty()));
    }

    #[test]
    fn resume_refuses_modified_bytes_and_cross_project_context() {
        for change in ["input", "result", "project", "lane", "transformer"] {
            reset();
            let temp = tempfile::tempdir().unwrap();
            let directory = temp.path().join("run");
            run_with(&inputs(&directory, &[]), &context(true), &MOCK).unwrap();
            let before = CALLS.with(|c| c.borrow().stages.len());
            if matches!(change, "input" | "result") {
                let file = if change == "input" {
                    "request.json"
                } else {
                    "result.json"
                };
                std::fs::write(directory.join(file), b"modified").unwrap();
            } else {
                let path = directory.join("source.json");
                let mut receipt: Value =
                    serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
                if change == "project" {
                    receipt["data"]["project"]["ds_project"] = json!("other");
                } else {
                    receipt["data"][change] = json!("other");
                }
                std::fs::write(path, receipt.to_string()).unwrap();
            }
            assert!(run_with(&inputs(&directory, &["--resume"]), &context(true), &MOCK).is_err());
            assert_eq!(CALLS.with(|c| c.borrow().stages.len()), before);
        }
    }

    #[test]
    fn operation_key_ignores_paths_and_changes_with_every_captured_fence() {
        let source = json!({"project":{"ds_project":"p"},"lane":"canary","transformer":"T1",
            "source":{"version":1,"content_digest":"digest"},"out":"path1"});
        let processed = json!({"input_sha256":"i","result_sha256":"r","out":"path1"});
        let original = operation_id(&source, &processed);
        let mut other = processed.clone();
        other["out"] = json!("path2");
        assert_eq!(operation_id(&source, &other), original);
        for field in ["input_sha256", "result_sha256"] {
            let mut other = processed.clone();
            other[field] = json!("changed");
            assert_ne!(operation_id(&source, &other), original);
        }
        for pointer in [
            "/project/ds_project",
            "/lane",
            "/transformer",
            "/source/version",
            "/source/content_digest",
        ] {
            let mut other = source.clone();
            *other.pointer_mut(pointer).unwrap() = json!("changed");
            assert_ne!(operation_id(&other, &processed), original);
        }
    }

    #[test]
    fn save_success_requires_identity_and_both_verified_digests() {
        let saved = json!({"project":"p","lane":"canary","result":{"project_id":"p","results":[{
            "transformer_name":"T1","verified":true,"version":8,
            "content_digest":"a".repeat(64),"analysis_sha256":"b".repeat(64)
        }]}});
        require_verified_save(&saved, "p", "T1", "canary").unwrap();
        for pointer in [
            "/project",
            "/lane",
            "/result/project_id",
            "/result/results/0/transformer_name",
            "/result/results/0/verified",
            "/result/results/0/version",
            "/result/results/0/content_digest",
            "/result/results/0/analysis_sha256",
        ] {
            let mut changed = saved.clone();
            *changed.pointer_mut(pointer).unwrap() = Value::Null;
            assert_eq!(
                require_verified_save(&changed, "p", "T1", "canary")
                    .unwrap_err()
                    .code(),
                "fast_lv_run_save_unverified"
            );
        }
        let mut changed = saved.clone();
        changed["result"]["results"][0]["error_code"] =
            json!("TRANSFORMER_SAVE_READBACK_UNVERIFIED");
        assert!(require_verified_save(&changed, "p", "T1", "canary").is_err());
        changed["result"]["results"] = json!([]);
        assert!(require_verified_save(&changed, "p", "T1", "canary").is_err());
    }

    #[test]
    fn replacing_a_receipt_between_checks_cannot_redirect_the_save_project() {
        fn replaced(inputs: &Inputs, context: &Context, project: &str) -> Result<Value, Failure> {
            let path = inputs.require("source")?;
            let mut receipt: Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
            receipt["data"]["project"]["ds_project"] = json!("other-project");
            std::fs::write(path, receipt.to_string()).unwrap();
            project_save::run_for_project(inputs, context, project)
        }
        reset();
        let temp = tempfile::tempdir().unwrap();
        let directory = temp.path().join("run");
        let owners = Owners {
            export,
            process: compute,
            save: replaced,
            ..MOCK
        };
        let refusal = run_with(&inputs(&directory, &[]), &context(true), &owners).unwrap_err();
        assert_eq!(refusal.code(), "fast_lv_save_input_invalid");
        assert!(refusal.message().contains("another explicit project"));
        assert!(!directory.join("save.json").exists());
    }
}
