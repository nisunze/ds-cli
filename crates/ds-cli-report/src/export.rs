//! `ds report export` — build a transformer or combined report.
//!
//! This is the command that shows what `ds` adds to a process contract it did
//! not write.
//!
//! Called directly, `ds-report` answers a failed export by *writing its result
//! document anyway* and exiting non-zero. That is the right design for the
//! engine — the blockers belong in a durable document, and an exit code
//! cannot carry them — but it leaves a caller holding an exit code and a
//! path, having to know that the interesting part is in the file.
//!
//! So `ds` reads the document in both outcomes and returns it. A failed
//! export becomes a typed refusal carrying the engine's own blockers as
//! structured data; a successful one becomes an envelope listing the
//! artifacts. Either way the answer is in the answer, and no caller has to
//! learn the convention.
//!
//! The engine's must-not-exist rule on the result path is honoured rather
//! than worked around. When the caller does not name a result path, `ds`
//! writes to a fresh file it owns and removes it afterwards — it never
//! deletes a path the caller chose.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Availability, Chapter, Command, Effect, Example, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Map, Value, json};

use crate::{DS_REPORT, EXPORT_TIMEOUT};

const TASKS: &[&str] = &["transformer", "combined", "voltage-drop", "lv-standard"];
const INPUT_SHAPES: &[&str] = &["firestore_rest", "plain_local"];

/// The engine subcommand behind each `--task`. Named here, in source, and
/// never assembled from caller input.
const TRANSFORMER_SUBCOMMAND: &str = "export-transformer-report";
const COMBINED_SUBCOMMAND: &str = "export-combined-transformer-report";
const VOLTAGE_DROP_SUBCOMMAND: &str = "render-voltage-drop-result";
const LV_STANDARD_SUBCOMMAND: &str = "export-lv-standard";

pub static COMMAND: Command = Command {
    id: "report.export",
    path: &["report", "export"],
    contract: 3,
    summary: "Export governed LV standard sets or local engineering reports.",
    purpose: "\
Builds report artifacts with the installed reporter engine. Reads only local \
bytes and makes no network call of any kind. The engine writes a result \
document describing every artifact and every blocker; this command returns \
that document, so a refused export arrives as typed blockers rather than an \
exit code and a file path. Use --request to supply the engine's full typed \
request instead of the flags below; run `ds report tasks --task <name>` for \
its schema. --task lv-standard requires the export_lv_standard JSON job: A0/A3 sheets, PNG before PDF and separate combined sets with six A0 opening pages. Governed defaults and overrides: report layout schema; details in the reference. --task voltage-drop requires --request from render_voltage_drop_result: it prints admitted calculated JSON or explicit reserved/incomplete/refused status to A4, without processing or inferring analysis. Governed identity supplies title blocks/logos. Report language comes from Network Template project_settings.report_locale (en/fr shipped; other locales require complete catalogue data). Missing language refuses; it is never inferred. PDF naming is owned by the kernel. Prints are regenerated; partial exports list failed_formats. A reporter without that task refuses; there is no export fallback.",
    chapter: Chapter::Reports,
    effect: Effect::LocalFileWrite,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[
        Arg::value("task", "<name>", "Which report to build.")
            .required()
            .choices(TASKS),
        Arg::value(
            "out-dir",
            "<path>",
            "Directory to write artifacts into; created if absent.",
        ),
        Arg::repeated(
            "transformer",
            "<name>",
            "Canonical transformer name. Once for --task transformer; repeat for combined.",
        ),
        Arg::value("network-config", "<path>", "Project configuration input."),
        Arg::repeated(
            "transformer-document",
            "<path>",
            "Transformer input document. Repeat in --transformer order for combined.",
        ),
        Arg::value(
            "input-shape",
            "<shape>",
            "How the input documents are shaped.",
        )
        .choices(INPUT_SHAPES),
        Arg::value(
            "country",
            "<name>",
            "Reporting country; anything but Rwanda scrubs admin columns.",
        ),
        Arg::repeated(
            "format",
            "<name>",
            "Restrict to a subset of the policy's export formats.",
        ),
        Arg::value(
            "admin-bounds",
            "<path>",
            "Rwanda villages asset (.dsab/.geojson/.geojson.zst).",
        ),
        Arg::value(
            "admin-bounds-sha256",
            "<hex>",
            "SHA-256 of the exact admin-bounds bytes.",
        ),
        Arg::value(
            "request",
            "<path>",
            "A complete engine request document. Mutually exclusive with the content flags.",
        ),
        Arg::value(
            "result",
            "<path>",
            "Keep the engine's result document here. Must not already exist.",
        ),
    ],
    output: "\
The engine's result document: status, artifacts produced, failed_formats and \
blockers; voltage-drop returns source/PDF digests, local PDF path, byte/page \
counts and a nothing-published receipt. `result_path` appears only when --result \
was given.",
    examples: &[
        Example {
            command: "ds report tasks --task export_transformer_report --output json",
            note: "See the engine's exact request schema before building one.",
            runnable: true,
        },
        Example {
            command: "ds report export --task transformer --transformer T-1 --transformer-document ./t.json --network-config ./config.json --out-dir ./out --output json",
            note: "Artifacts land in ./out; the result document is returned inline.",
            runnable: false,
        },
        Example {
            command: "ds report export --task combined --request ./request.json --result ./result.json --output json",
            note: "Full typed request, result document kept on disk.",
            runnable: false,
        },
    ],
    refusals: &[
        Refusal {
            code: "report_locale_missing",
            when: "the reporter refuses this presentation contract",
            remedy: "set report_locale in the existing Network Template project_settings sheet; backfill dry run first",
        },
        Refusal {
            code: "report_locale_invalid",
            when: "the reporter refuses this presentation contract",
            remedy: "use one exact nonempty report locale key in project_settings",
        },
        Refusal {
            code: "report_locale_unsupported",
            when: "the reporter refuses this presentation contract",
            remedy: "supply the complete locale in the one report message catalogue",
        },
        Refusal {
            code: "report_catalogue_invalid",
            when: "the reporter refuses this presentation contract",
            remedy: "supply ds.report-messages/v1 with an English reference locale",
        },
        Refusal {
            code: "report_catalogue_incomplete",
            when: "the reporter refuses this presentation contract",
            remedy: "complete every message and preserve named placeholders",
        },
        Refusal {
            code: "report_message_missing",
            when: "the reporter refuses this presentation contract",
            remedy: "add the message to every locale in the catalogue",
        },
        Refusal {
            code: "report_header_alias_invalid",
            when: "the reporter refuses this presentation contract",
            remedy: "fix ambiguous cleaned_header/misspelled_header mappings in the Network Template",
        },
        Refusal {
            code: "unknown_task",
            when: "the installed reporter does not publish the selected voltage-drop or LV standard task",
            remedy: "install a reporter exposing the selected task in report tasks; no alternate rendering or recomputation fallback is permitted",
        },
        Refusal {
            code: "reporter_engine_missing",
            when: "`ds-report` is not installed next to `ds`",
            remedy: "install the desktop, or set DS_REPORT_BIN to a built ds-report",
        },
        Refusal {
            code: "conflicting_inputs",
            when: "--request was given alongside a content flag",
            remedy: "pass either --request or the content flags, not both",
        },
        Refusal {
            code: "missing_input",
            when: "a field the chosen task requires was not supplied",
            remedy: "run `ds report tasks --task <name>` for its required fields",
        },
        Refusal {
            code: "too_many_transformers",
            when: "--task transformer was given more than one --transformer",
            remedy: "pass --transformer once, or use --task combined for several",
        },
        Refusal {
            code: "transformer_document_mismatch",
            when: "combined transformer names and documents have different counts",
            remedy: "pair each --transformer with one --transformer-document in the same order",
        },
        Refusal {
            code: "request_not_found",
            when: "--request does not name a readable file",
            remedy: "check the path, or build the request from the content flags",
        },
        Refusal {
            code: "scratch_unwritable",
            when: "the temporary directory would not accept the staged request",
            remedy: "check that TMPDIR exists and is writable",
        },
        Refusal {
            code: "result_exists",
            when: "--result names a path that already exists",
            remedy: "choose a new path; the engine has no --force, on purpose",
        },
        Refusal {
            code: "export_blocked",
            when: "the engine ran and refused; its blockers are in the refusal detail",
            remedy: "read detail.blockers, fix the inputs, and retry",
        },
        Refusal {
            code: "engine_refused",
            when: "the engine failed before producing a document",
            remedy: "read detail.engine for the engine's own message",
        },
    ],
    reference: Some("docs/reference/report.md"),
    search: &["voltage drop", "calculated json", "headless chromium"],
    requires: Requires::Server,
    availability,
};

fn availability() -> Availability {
    DS_REPORT.availability()
}

/// The content flags, so "did the caller mix --request with content" is
/// answered from one list rather than a forgotten `if`.
const CONTENT_FLAGS: &[&str] = &[
    "out-dir",
    "transformer",
    "network-config",
    "transformer-document",
    "input-shape",
    "country",
    "format",
    "admin-bounds",
    "admin-bounds-sha256",
];

/// Discover the exact owner task before any project compute/save effect.
pub fn voltage_drop_preflight() -> Result<(), Failure> {
    require_task("render_voltage_drop_result", VOLTAGE_DROP_SUBCOMMAND)
}

/// Inspect the workstation-owned browser selection without effects. The
/// composing command owns its refusal code and remedy.
pub fn voltage_drop_browser_preflight() -> Result<(), String> {
    let selected = ds_cli_workstation::policy::read_browser_selection(
        ds_cli_workstation::detect::Platform::current(),
    )?
    .ok_or_else(|| "No verified reporter browser is configured.".to_owned())?;
    if !Path::new(&selected.executable).is_file() {
        return Err("The configured reporter browser is missing.".to_owned());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if std::fs::metadata(&selected.executable)
            .map_err(|error| error.to_string())?
            .permissions()
            .mode()
            & 0o111
            == 0
        {
            return Err("The configured reporter browser is not executable.".to_owned());
        }
    }
    Ok(())
}

fn require_task(name: &str, subcommand: &str) -> Result<(), Failure> {
    let schemas = crate::tasks::schemas()?;
    if !schemas["tasks"].as_array().is_some_and(|tasks| {
        tasks
            .iter()
            .any(|task| task["name"] == name && task["subcommand"] == subcommand)
    }) {
        return Err(Failure::unavailable("unknown_task", format!("the installed reporter does not expose {name}"))
            .remedy(format!("install a reporter exposing {name}; no alternate rendering or recomputation fallback is permitted")));
    }
    Ok(())
}

/// Verify the owner's receipt against its local artifact. No rendering or analysis here.
pub fn verify_voltage_drop_pdf(
    receipt: &Value,
    pdf: &Path,
    source_sha: &str,
    transformer: &str,
    project_label: &str,
) -> Result<(), String> {
    use sha2::{Digest, Sha256};
    let invalid = || "The reporter PDF does not match its exact receipt.".to_owned();
    let metadata = std::fs::symlink_metadata(pdf).map_err(|_| invalid())?;
    if !metadata.is_file() || metadata.file_type().is_symlink() || metadata.len() > 64 * 1024 * 1024
    {
        return Err(invalid());
    }
    use std::io::Read;
    let mut bytes = Vec::new();
    std::fs::File::open(pdf)
        .and_then(|file| file.take(64 * 1024 * 1024 + 1).read_to_end(&mut bytes))
        .map_err(|_| invalid())?;
    if bytes.len() > 64 * 1024 * 1024 {
        return Err(invalid());
    }
    let pages = lopdf::Document::load_mem(&bytes)
        .map_err(|_| invalid())?
        .get_pages()
        .len();
    if receipt["schema"] != "ds.voltage-drop-pdf.render/v1"
        || receipt["transformer"] != transformer
        || receipt["project_label"] != project_label
        || receipt["source_sha256"] != source_sha
        || receipt["out_pdf"] != pdf.to_string_lossy().as_ref()
        || receipt["output_sha256"] != format!("{:x}", Sha256::digest(&bytes))
        || receipt["bytes"].as_u64() != Some(bytes.len() as u64)
        || pages == 0
        || receipt["pages"].as_u64() != Some(pages as u64)
        || receipt["publication"] != "nothing_published"
    {
        return Err(invalid());
    }
    Ok(())
}

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let task = inputs.require("task")?;
    let subcommand = match task {
        "transformer" => TRANSFORMER_SUBCOMMAND,
        "combined" => COMBINED_SUBCOMMAND,
        "voltage-drop" => VOLTAGE_DROP_SUBCOMMAND,
        "lv-standard" => LV_STANDARD_SUBCOMMAND,
        other => {
            return Err(Failure::internal(
                "unmapped_task",
                format!("`--task {other}` passed validation but maps to no engine subcommand"),
            ));
        }
    };

    let supplied_request = inputs.value("request");
    let used_content: Vec<&str> = CONTENT_FLAGS
        .iter()
        .copied()
        .filter(|flag| inputs.value(flag).is_some() || !inputs.repeated(flag).is_empty())
        .collect();

    if supplied_request.is_some() && !used_content.is_empty() {
        return Err(Failure::invalid(
            "conflicting_inputs",
            "--request carries the whole request; the content flags would be ignored",
        )
        .remedy("pass either --request or the content flags, not both")
        .detail(json!({ "conflicting": used_content })));
    }

    if task == "lv-standard" && supplied_request.is_none() {
        return Err(Failure::invalid(
            "missing_input",
            "lv-standard requires its declarative JSON job",
        )
        .remedy("discover ds report tasks --task export_lv_standard and pass --request"));
    }

    if task == "voltage-drop" {
        if supplied_request.is_none() {
            return Err(Failure::invalid(
                "missing_input",
                "--task voltage-drop requires --request; content flags cannot supply calculated analysis",
            )
            .remedy("discover `ds report tasks --task render_voltage_drop_result` and supply its exact request"));
        }
        require_task("render_voltage_drop_result", VOLTAGE_DROP_SUBCOMMAND)?;
    }

    if task == "lv-standard" {
        require_task("export_lv_standard", LV_STANDARD_SUBCOMMAND)?;
    }

    // Where the engine's result document goes. A caller-named path is theirs
    // — checked, used, and never removed. Otherwise `ds` owns a scratch file
    // and cleans it up.
    let (result_path, caller_owned) = match inputs.value("result") {
        Some(path) => {
            let path = PathBuf::from(path);
            if path.symlink_metadata().is_ok() {
                return Err(Failure::new(
                    ds_cli_contract::ExitClass::Conflict,
                    "result_exists",
                    format!("`{}` already exists", path.display()),
                )
                .remedy("choose a new path; the engine has no --force, on purpose"));
            }
            (path, true)
        }
        None => (scratch_path("result"), false),
    };

    // Likewise for the request document: a caller-supplied one is used as-is,
    // and a constructed one is written to scratch and removed.
    let (request_path, request_owned) = match supplied_request {
        Some(path) => {
            let path = PathBuf::from(path);
            if !path.is_file() {
                return Err(Failure::invalid(
                    "request_not_found",
                    format!("cannot read the request at `{}`", path.display()),
                )
                .remedy("check the path, or build the request from the content flags"));
            }
            (path, true)
        }
        None => {
            let request = build_request(task, inputs)?;
            let path = scratch_path("request");
            write_new(&path, &serde_json::to_vec(&request).unwrap_or_default())?;
            (path, false)
        }
    };

    let args: Vec<OsString> = vec![
        OsString::from("--request"),
        request_path.clone().into(),
        OsString::from("--result"),
        result_path.clone().into(),
    ];

    let completed = DS_REPORT.call(subcommand, &args, EXPORT_TIMEOUT);
    if !request_owned {
        let _ = std::fs::remove_file(&request_path);
    }
    let completed = completed?;

    // The engine writes its document whether it succeeded or not, so read it
    // before looking at the exit code. This is the whole point of the command.
    let document = std::fs::read(&result_path)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok());
    if !caller_owned {
        let _ = std::fs::remove_file(&result_path);
    }

    let Some(mut document) = document else {
        // No document at all means the engine failed before doing any work —
        // a bad request, a missing input file, an unusable output directory.
        if let Some(code) = ds_command_kernel::report_messages::refusal_code(&completed.stderr) {
            let failure = match code {
                "report_locale_missing"
                | "report_locale_invalid"
                | "report_locale_unsupported"
                | "report_catalogue_invalid"
                | "report_catalogue_incomplete"
                | "report_message_missing"
                | "report_header_alias_invalid" => {
                    Failure::invalid(code, "Report locale or catalogue refused")
                }
                _ => Failure::invalid("engine_refused", "Reporter refused the request"),
            };
            return Err(failure.remedy("Use project_settings.report_locale and a complete report catalogue; inspect the named refusal").detail(json!({"engine":completed.stderr})));
        }
        return Err(DS_REPORT.failure_from(&completed, subcommand));
    };

    if let (true, Some(object)) = (caller_owned, document.as_object_mut()) {
        object.insert(
            "result_path".into(),
            json!(result_path.display().to_string()),
        );
    }

    if completed.succeeded() {
        return Ok(document);
    }

    Err(Failure::failed(
        "export_blocked",
        "the engine ran and refused to produce the report",
    )
    .remedy("read detail.blockers, fix the inputs, and retry")
    .next("ds report tasks --task <name>")
    .detail(json!({
        "status": document.get("status"),
        "code": document.get("code"),
        "reason": document.get("reason"),
        "blockers": document.get("blockers"),
        "artifacts": document.get("artifacts").map(|artifacts| {
            artifacts.as_array().map_or(0, Vec::len)
        }),
        "result_path": caller_owned.then(|| result_path.display().to_string()),
    })))
}

/// Translate the declared flags into the engine's typed request.
///
/// The field names here are the engine's, from its published schema. They are
/// hand-authored and *checked*: `crates/ds/tests/engine_parity.rs` fetches
/// `ds-report task-schemas` from the installed engine and asserts every
/// required field of each task is reachable from this command's flags. An
/// unchecked hand copy drifts silently, which is worse than no copy.
fn build_request(task: &str, inputs: &Inputs) -> Result<Value, Failure> {
    let mut request = Map::new();

    let out_dir = required(inputs, "out-dir", "out_dir")?;
    request.insert("out_dir".into(), json!(out_dir));
    let network_config = required(inputs, "network-config", "network_config")?;
    request.insert("network_config".into(), json!(network_config));

    let transformers = inputs.repeated("transformer");
    let documents = inputs.repeated("transformer-document");
    match task {
        "transformer" => {
            let name = match transformers {
                [one] => one.clone(),
                [] => {
                    return Err(missing("transformer", "transformer"));
                }
                many => {
                    return Err(Failure::invalid(
                        "too_many_transformers",
                        "--task transformer builds one report; pass --transformer once",
                    )
                    .remedy("use --task combined for several transformers")
                    .detail(json!({ "given": many.len() })));
                }
            };
            request.insert("transformer".into(), json!(name));
            let document = match documents {
                [one] => one,
                [] => return Err(missing("transformer-document", "transformer_document")),
                many => {
                    return Err(Failure::invalid(
                        "transformer_document_mismatch",
                        "--task transformer accepts one --transformer-document",
                    )
                    .detail(json!({ "given": many.len() })));
                }
            };
            request.insert("transformer_document".into(), json!(document));
        }
        _ => {
            if transformers.is_empty() {
                return Err(missing("transformer", "transformers"));
            }
            if transformers.len() != documents.len() {
                return Err(Failure::invalid(
                    "transformer_document_mismatch",
                    "combined export needs one document for each transformer",
                )
                .remedy("pair each --transformer with one --transformer-document in the same order")
                .detail(
                    json!({ "transformers": transformers.len(), "documents": documents.len() }),
                ));
            }
            request.insert(
                "transformers".into(),
                Value::Array(
                    transformers
                        .iter()
                        .zip(documents.iter())
                        .map(|(transformer, layers)| json!({ "transformer": transformer, "layers": layers }))
                        .collect(),
                ),
            );
        }
    }

    for (flag, field) in [
        ("country", "country"),
        ("admin-bounds", "admin_bounds_asset"),
        ("admin-bounds-sha256", "admin_bounds_sha256"),
    ] {
        if let Some(value) = inputs.value(flag) {
            request.insert(field.into(), json!(value));
        }
    }

    if task == "transformer"
        && let Some(value) = inputs.value("input-shape")
    {
        request.insert("input_shape".into(), json!(value));
    }

    let formats = inputs.repeated("format");
    if task == "transformer" && !formats.is_empty() {
        request.insert("formats".into(), json!(formats));
    }

    Ok(Value::Object(request))
}

fn required<'a>(inputs: &'a Inputs, flag: &str, field: &str) -> Result<&'a str, Failure> {
    inputs.value(flag).ok_or_else(|| missing(flag, field))
}

fn missing(flag: &str, field: &str) -> Failure {
    Failure::invalid(
        "missing_input",
        format!("the engine requires `{field}`; pass `--{flag}`"),
    )
    .remedy("run `ds report tasks --task <name>` for its required fields")
    .next("ds report tasks")
}

/// A scratch path this process owns. The engine refuses a result path that
/// already exists, so uniqueness is a correctness requirement, not tidiness.
fn scratch_path(kind: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_nanos())
        .unwrap_or_default();
    std::env::temp_dir().join(format!(
        "ds-report-{kind}-{}-{nanos}.json",
        std::process::id()
    ))
}

fn write_new(path: &Path, bytes: &[u8]) -> Result<(), Failure> {
    use std::io::Write as _;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path).map_err(|error| {
        Failure::failed(
            "scratch_unwritable",
            "could not stage the engine request in the temporary directory",
        )
        .remedy("check TMPDIR is writable")
        .detail(json!({ "detail": error.kind().to_string() }))
    })?;
    file.write_all(bytes).map_err(|error| {
        Failure::failed("scratch_unwritable", "could not write the engine request")
            .detail(json!({ "detail": error.kind().to_string() }))
    })
}

pub fn render(data: &Value) -> String {
    if data["schema"] == "ds.voltage-drop-pdf.render/v1" {
        return format!(
            "{} A4 page(s) — {}\n{}\nsource SHA-256: {}\n{}",
            data["pages"],
            data["transformer"].as_str().unwrap_or(""),
            data["out_pdf"].as_str().unwrap_or(""),
            data["source_sha256"].as_str().unwrap_or(""),
            data["publication"].as_str().unwrap_or(""),
        );
    }
    let artifacts = data["artifacts"].as_array().map_or(0, Vec::len);
    let blockers = data["blockers"].as_array().map_or(0, Vec::len);
    let mut out = format!(
        "{} — {artifacts} artifact(s), {blockers} blocker(s)\n",
        data["status"].as_str().unwrap_or("?"),
    );
    for artifact in data["artifacts"].as_array().into_iter().flatten() {
        let path = artifact
            .get("path")
            .and_then(Value::as_str)
            .or_else(|| artifact.as_str())
            .unwrap_or("");
        out.push_str(&format!("  {path}\n"));
    }
    if let Some(path) = data["result_path"].as_str() {
        out.push_str(&format!("\nresult document: {path}\n"));
    }
    out
}

#[cfg(test)]
mod voltage_drop_tests {
    use super::*;
    use sha2::{Digest, Sha256};

    #[test]
    fn pdf_receipt_must_prove_identity_exact_digest_bytes_and_actual_pages() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("a4.pdf");
        let mut doc = lopdf::Document::with_version("1.5");
        let pages = doc.new_object_id();
        let page = doc.add_object(lopdf::dictionary! {"Type" => "Page", "Parent" => pages,
        "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()]});
        doc.objects.insert(
            pages,
            lopdf::dictionary! {"Type" => "Pages", "Kids" => vec![page.into()], "Count" => 1}
                .into(),
        );
        let catalog = doc.add_object(lopdf::dictionary! {"Type" => "Catalog", "Pages" => pages});
        doc.trailer.set("Root", catalog);
        let mut bytes = Vec::new();
        doc.save_to(&mut bytes).unwrap();
        std::fs::write(&path, &bytes).unwrap();
        let sha = "a".repeat(64);
        let receipt = json!({"schema":"ds.voltage-drop-pdf.render/v1","out_pdf":path,"source_sha256":sha,
            "transformer":"T1","project_label":"project-a","bytes":bytes.len(),"pages":1,
            "output_sha256":format!("{:x}",Sha256::digest(&bytes)),"publication":"nothing_published"});
        verify_voltage_drop_pdf(&receipt, &path, &sha, "T1", "project-a").unwrap();
        for key in [
            "schema",
            "out_pdf",
            "source_sha256",
            "transformer",
            "project_label",
            "bytes",
            "pages",
            "output_sha256",
            "publication",
        ] {
            let mut bad = receipt.clone();
            bad[key] = Value::Null;
            assert_eq!(
                verify_voltage_drop_pdf(&bad, &path, &sha, "T1", "project-a").unwrap_err(),
                "The reporter PDF does not match its exact receipt."
            );
        }
        let mut bad = receipt.clone();
        bad["pages"] = json!(2);
        assert!(verify_voltage_drop_pdf(&bad, &path, &sha, "T1", "project-a").is_err());
        std::fs::write(&path, b"%PDF-truncated").unwrap();
        assert!(verify_voltage_drop_pdf(&receipt, &path, &sha, "T1", "project-a").is_err());
    }
}
