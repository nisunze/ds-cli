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
    Arg, Authority, Availability, Chapter, Command, Effect, Example, Execution, Refusal,
};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Map, Value, json};

use crate::{DS_REPORT, EXPORT_TIMEOUT};

const TASKS: &[&str] = &["transformer", "combined"];
const INPUT_SHAPES: &[&str] = &["firestore_rest", "plain_local"];

/// The engine subcommand behind each `--task`. Named here, in source, and
/// never assembled from caller input.
const TRANSFORMER_SUBCOMMAND: &str = "export-transformer-report";
const COMBINED_SUBCOMMAND: &str = "export-combined-transformer-report";

pub static COMMAND: Command = Command {
    id: "report.export",
    path: &["report", "export"],
    contract: 1,
    summary: "Export a transformer or combined report from local inputs.",
    purpose: "\
Builds report artifacts with the installed reporter engine. Reads only local \
bytes and makes no network call of any kind. The engine writes a result \
document describing every artifact and every blocker; this command returns \
that document, so a refused export arrives as typed blockers rather than an \
exit code and a file path. Use --request to supply the engine's full typed \
request instead of the flags below; run `ds report tasks --task <name>` for \
its schema.",
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
The engine's own result document: status, the artifacts it produced, and any \
blockers. `result_path` is present only when --result was given.",
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

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let task = inputs.require("task")?;
    let subcommand = match task {
        "transformer" => TRANSFORMER_SUBCOMMAND,
        "combined" => COMBINED_SUBCOMMAND,
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
