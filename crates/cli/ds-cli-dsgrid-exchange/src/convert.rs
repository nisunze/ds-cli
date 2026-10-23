//! `ds dsgrid-exchange convert` — plan it, then write exactly that plan.
//!
//! This is the only command in the domain that writes. It plans first,
//! refuses a plan carrying any blocker, executes, and materializes the
//! artifacts under `--out` alongside an `exchange-report.json`.
//!
//! Three properties are worth stating because they are the reason this is
//! safe to hand to an agent:
//!
//! * **The plan is re-pinned.** `execute_conversion` re-digests every source
//!   and refuses if the bytes differ from what the plan pinned. A file edited
//!   between `plan` and `convert` fails loudly instead of converting
//!   something nobody previewed.
//! * **The artifact plan owns path identity.** Output paths come from the
//!   plan. This command will not mint a `1-` prefix, flatten a nested
//!   library, or resolve a case-only collision — it refuses, because
//!   materializing bytes at a path the plan did not name means the verified
//!   plan and the written tree are different documents.
//! * **It never overwrites.** An existing output path is a refusal, not a
//!   silent replacement. There is deliberately no `--force`: the remedy is a
//!   new directory, which keeps the previous conversion's evidence intact.
//! * **The archive is verified before anything is written.** The Network
//!   addon plans and runs the conversion (DECOUPLING-003) and returns every
//!   output in one stored archive with its declared path, size and digest.
//!   Each entry must match its declaration and stay inside `--out`; an
//!   archive that does not is refused before the first byte lands.

use std::collections::BTreeSet;
use std::io::Read as _;
use std::path::{Component, Path, PathBuf};

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Availability, Chapter, Command, Effect, Example, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Inputs};
use ds_cli_dsgrid::native;
use ds_network_contracts::grid_exchange::{
    EXCHANGE_OUTPUTS_ARTIFACT, ExchangeConvertResultV1, ExchangeOutputV1, ExchangeRequestParamsV1,
};
use ds_network_contracts::grid_operation::GridOperationV1;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::{refusals, render, request, sources};

/// Artifacts listed in the result. The full count is always reported; a
/// workspace conversion can write thousands of members and the list is not
/// the answer — the digests and the report path are.
const WRITTEN_LIMIT: usize = 25;

pub static COMMAND: Command = Command {
    id: "dsgrid-exchange.convert",
    path: &["dsgrid-exchange", "convert"],
    contract: 1,
    summary: "Execute a conversion and write its artifacts and exchange report.",
    purpose: "\
Plans the conversion, refuses if the plan carries a blocker, then executes it \
and writes every artifact under --out together with an exchange-report.json. \
Source bytes are re-digested against the plan's pins before anything is \
written, so a source that changed since planning stops the run. Existing \
output paths are never overwritten. A kmz or shp is a client GIS file: it \
leaves out tension_sections and terrain_points unless --include-layer names \
them, and its engine report is written beside it, never inside. A conversion \
that runs but does not complete is not a success: it fails as \
conversion_failed or conversion_partial after writing its exchange report.",
    chapter: Chapter::GridModel,
    effect: Effect::LocalFileWrite,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &ARGS,
    output: "\
The plan id, the outcome status, per-source status with snapshot \
fingerprints, and each written artifact with its sha256 — plus the path of \
the exchange report.",
    examples: &[
        Example {
            command: "ds dsgrid-exchange convert --source ./rev3.dsgrid --source ./school.dsgrid --target kmz --mode merge-gis --out ./out --output json",
            note: "Combine GIS features without reconciling or modifying the source engineering libraries.",
            runnable: false,
        },
        Example {
            command: "ds dsgrid-exchange convert --source ./workspace --target dsgrid --out ./out --output json",
            note: "Import a PLS-CADD workspace into a canonical .dsgrid.",
            runnable: false,
        },
        Example {
            command: "ds dsgrid-exchange convert --source ./model.dsgrid --target kmz --out ./out --output json",
            note: "Export a model for GIS, after reading the losses `plan` reported.",
            runnable: false,
        },
        Example {
            command: "ds dsgrid-exchange convert --source ./model.dsgrid --target shp --include-layer tension_sections --out ./out --output json",
            note: "A client Shapefile ZIP that keeps tension sections; terrain points stay out.",
            runnable: false,
        },
        Example {
            command: "ds dsgrid-exchange convert --source ./network.zip --target dsgrid --alignment-layer mv_lines --alignment-label-property node_id --network-source-layer site_solaires --network-source-role plant --crs EPSG:32633 --out ./out --output json",
            note: "Create a source-rooted MV skeleton from GIS and retain the complete source as package context.",
            runnable: false,
        },
    ],
    refusals: &REFUSALS,
    reference: Some("docs/reference/dsgrid-exchange.md"),
    search: &["combine gis", "merged kmz", "merged shapefile"],
    requires: Requires::Server,
    availability: available,
};

/// `plan`'s inputs plus the output directory. Declared by splicing rather
/// than restating, because "convert takes exactly what plan takes" is the
/// property that makes previewing worth doing.
static ARGS: [Arg; request::SHARED_ARGS.len() + 1] = args();

const fn args() -> [Arg; request::SHARED_ARGS.len() + 1] {
    let mut out = [Arg::switch("", ""); request::SHARED_ARGS.len() + 1];
    let mut index = 0;
    while index < request::SHARED_ARGS.len() {
        out[index] = request::SHARED_ARGS[index];
        index += 1;
    }
    out[index] = Arg::value(
        "out",
        "<dir>",
        "Directory to write artifacts and the exchange report into.",
    )
    .required();
    out
}

static REFUSALS: [Refusal; 21 + native::REFUSALS.len()] = refusals::splice(&[
    sources::SHARED_REFUSALS,
    request::REQUEST_REFUSALS,
    CONVERT_REFUSALS,
    native::REFUSALS,
]);

/// Exactly the codes `run` and its helpers emit — no more. A declared refusal
/// nothing can raise is as misleading as an undeclared one that can: both
/// leave a caller planning for the wrong failures.
const CONVERT_REFUSALS: &[Refusal] = &[
    Refusal {
        code: "conversion_plan_choices_mismatch",
        when: "execution choices differ from the immutable plan ID",
        remedy: "preview a new plan with the exact intended choices",
    },
    Refusal {
        code: "plan_blocked",
        when: "the conversion plan carries at least one blocker",
        remedy: "run `ds dsgrid-exchange plan` with the same flags and resolve each blocker",
    },
    Refusal {
        code: "source_digest_mismatch",
        when: "a source's bytes changed between planning and execution",
        remedy: "re-run; the plan pins the bytes it previewed and will not convert different ones",
    },
    Refusal {
        code: "output_path_collision",
        when: "the plan emits the same output path twice",
        remedy: "report this: a plan that names one path twice is an engine defect, not an input error",
    },
    Refusal {
        code: "output_exists",
        when: "an artifact path already exists under --out",
        remedy: "convert into a new directory; this command never overwrites and has no --force",
    },
    Refusal {
        code: "output_unwritable",
        when: "--out cannot be created or written",
        remedy: "check the path and its permissions",
    },
    Refusal {
        code: "report_unserializable",
        when: "the exchange report could not be encoded",
        remedy: "report this: the artifacts were written, but their evidence document was not",
    },
    Refusal {
        code: "conversion_failed",
        when: "the plan ran but no source produced its target artifact",
        remedy: "read each source's detail and the exchange report written under --out; resolve the named cause, plan again and convert into a new directory",
    },
    Refusal {
        code: "conversion_partial",
        when: "the plan ran and some sources converted while others failed",
        remedy: "the written artifacts and the exchange report stay as evidence; resolve each failed source's detail and convert those sources into a new directory",
    },
];

fn available() -> Availability {
    Availability::Available
}

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let out_dir = PathBuf::from(inputs.require("out")?);
    let loaded = sources::load(inputs.repeated("source"))?;
    // The absolute paths the sources were read from, in `--source` order.
    // They travel into the exchange report beside the pinned digests so a
    // later `ds dsgrid model import-external` of this output can link the
    // working copy to the workspace it came from without the operator
    // re-typing it (contract 02 §1).
    let source_paths = inputs
        .repeated("source")
        .iter()
        .map(|raw| absolute_path(raw))
        .collect::<Vec<_>>();
    let mut outcome = crate::exchange(
        &GridOperationV1::ExchangeConvert(ExchangeRequestParamsV1 {
            sources: loaded.sources,
            choices: request::choices(inputs),
            warnings: loaded.warnings,
        }),
        &loaded.inputs,
    )?;
    let archive = outcome.take_artifact(EXCHANGE_OUTPUTS_ARTIFACT);
    let converted: ExchangeConvertResultV1 = crate::result(outcome)?;
    let files = verified_outputs(&converted.outputs, archive)?;
    let written = materialize(&out_dir, &converted.outputs, files)?;
    let report_path = write_report(&out_dir, converted.report, &source_paths)?;
    finish(
        &converted.status,
        project(converted.answer, written, &report_path),
    )
}

/// Only a completed outcome is a success.
///
/// A conversion that ran and failed, wholly or for some sources, has already
/// written what it produced and the exchange report: those are the evidence a
/// reader needs. The envelope still has to say the work did not happen, or a
/// caller (and every reliability report built on the exit class) would count
/// a backup that was never written as delivered.
fn finish(status: &str, answer: Value) -> Result<Value, Failure> {
    let failure = match status {
        "completed" => return Ok(answer),
        "failed" => Failure::failed(
            "conversion_failed",
            "the conversion ran but no source produced its target artifact",
        )
        .remedy(
            "read each source's detail and the exchange report under --out; resolve the named cause, plan again and convert into a new directory",
        ),
        "partially_completed" => Failure::failed(
            "conversion_partial",
            "the conversion ran but only some sources produced their target artifacts",
        )
        .remedy(
            "keep the written artifacts and report as evidence; resolve each failed source's detail and convert those sources into a new directory",
        ),
        other => {
            return Err(Failure::failed(
                "network_result_invalid",
                format!("the conversion reported the unknown status `{other}`"),
            )
            .detail(answer));
        }
    };
    Err(failure
        .next("ds dsgrid-exchange plan --source <path> --target <format>")
        .detail(answer))
}

fn invalid_archive(message: impl Into<String>) -> Failure {
    Failure::failed("network_result_invalid", message.into())
        .remedy("use an intact compatible addon and unchanged inputs")
}

/// A plan-relative output path stays a plain relative path: no root, prefix,
/// `.` or `..` component, so joining it under `--out` cannot leave it.
fn contained(relative: &str) -> bool {
    let path = Path::new(relative);
    !relative.is_empty()
        && path
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
}

/// Every declared output's bytes from the returned archive, verified against
/// its declaration: present once, exact size and digest, and nothing else in
/// the archive. Nothing is written until all of them pass.
fn verified_outputs(
    outputs: &[ExchangeOutputV1],
    archive: Option<Vec<u8>>,
) -> Result<Vec<Vec<u8>>, Failure> {
    if outputs.is_empty() {
        return match archive {
            None => Ok(Vec::new()),
            Some(_) => Err(invalid_archive(
                "the conversion returned outputs it did not declare",
            )),
        };
    }
    let archive =
        archive.ok_or_else(|| invalid_archive("the conversion declared outputs but returned none"))?;
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(archive))
        .map_err(|error| invalid_archive(format!("the outputs archive is unreadable: {error}")))?;
    if archive.len() != outputs.len() {
        return Err(invalid_archive(format!(
            "the outputs archive holds {} entries for {} declared outputs",
            archive.len(),
            outputs.len()
        )));
    }
    let mut files = Vec::with_capacity(outputs.len());
    for output in outputs {
        if !contained(&output.path) {
            return Err(invalid_archive(format!(
                "the output path `{}` is not a plain relative path",
                output.path
            )));
        }
        let mut entry = archive
            .by_name(&output.path)
            .map_err(|_| invalid_archive(format!("`{}` is missing from the outputs", output.path)))?;
        let mut bytes = Vec::with_capacity(usize::try_from(output.byte_len).unwrap_or(0));
        entry
            .read_to_end(&mut bytes)
            .map_err(|error| invalid_archive(format!("`{}`: {error}", output.path)))?;
        if bytes.len() as u64 != output.byte_len || sha256(&bytes) != output.sha256 {
            return Err(invalid_archive(format!(
                "`{}` differs from its declared size or digest",
                output.path
            )));
        }
        files.push(bytes);
    }
    Ok(files)
}

/// Write every verified output under `out_dir`.
///
/// Paths come from the plan verbatim. The collision check is a defect
/// detector, not a repair: minting a distinct name would materialize a tree
/// the plan does not describe.
fn materialize(
    out_dir: &Path,
    outputs: &[ExchangeOutputV1],
    files: Vec<Vec<u8>>,
) -> Result<Vec<Value>, Failure> {
    create_dir(out_dir)?;

    let mut claimed: BTreeSet<String> = BTreeSet::new();
    // Refuse every collision or existing path before the first write, so a
    // refused conversion leaves `--out` as it found it.
    for output in outputs {
        let relative = &output.path;
        // Case-insensitive, because Windows and macOS treat two paths
        // differing only in case as one file. A collision that only appears
        // on the operator's filesystem is a collision this must catch here.
        if !claimed.insert(relative.to_ascii_lowercase()) {
            return Err(Failure::internal(
                "output_path_collision",
                format!("the plan emits `{relative}` more than once"),
            )
            .remedy("report this: a plan naming one path twice is an engine defect")
            .detail(json!({ "path": relative })));
        }
        let target = out_dir.join(relative);
        if target.exists() {
            return Err(Failure::conflict(
                "output_exists",
                format!("`{}` already exists", target.display()),
            )
            .remedy("convert into a new directory; this command never overwrites")
            .detail(json!({ "path": relative })));
        }
    }

    let mut written = Vec::with_capacity(outputs.len());
    for (output, bytes) in outputs.iter().zip(files) {
        let target = out_dir.join(&output.path);
        if let Some(parent) = target.parent() {
            create_dir(parent)?;
        }
        write_bytes(&target, &bytes)?;
        written.push(json!({
            "path": output.path,
            "artifact_type": output.artifact_type,
            "byte_len": output.byte_len,
            "sha256": output.sha256,
            "source_index": output.source_index,
        }));
    }
    Ok(written)
}

/// The path a source was named by, made absolute against the working
/// directory. Not canonicalized: a Drive stream or a mapped share may resolve
/// to a name the operator would not recognise, and the link is for them.
fn absolute_path(raw: &str) -> String {
    let path = Path::new(raw);
    if path.is_absolute() {
        return raw.to_string();
    }
    std::env::current_dir()
        .map(|cwd| cwd.join(path).to_string_lossy().into_owned())
        .unwrap_or_else(|_| raw.to_string())
}

/// Write the exchange report beside the artifacts.
///
/// The report carries the plan and the per-source outcome together, because
/// the pair is the evidence: the plan says what was promised, the outcome
/// says what happened, and a reader comparing them needs both in one file.
/// The owner's report names each source by index; the host adds the path it
/// read that source from.
fn write_report(
    out_dir: &Path,
    mut document: Value,
    source_paths: &[String],
) -> Result<PathBuf, Failure> {
    let path = out_dir.join("exchange-report.json");
    if path.exists() {
        return Err(Failure::conflict(
            "output_exists",
            format!("`{}` already exists", path.display()),
        )
        .remedy("convert into a new directory; this command never overwrites"));
    }
    if let Some(rows) = document.get_mut("sources").and_then(Value::as_array_mut) {
        for row in rows {
            let index = row["source_index"].as_u64().map(|index| index as usize);
            if let Some(object) = row.as_object_mut() {
                object.insert(
                    "path".into(),
                    json!(index.and_then(|index| source_paths.get(index))),
                );
            }
        }
    }
    let bytes = serde_json::to_vec_pretty(&document).map_err(|error| {
        Failure::internal(
            "report_unserializable",
            "the exchange report did not serialize",
        )
        .detail(json!({ "detail": error.to_string() }))
    })?;
    write_bytes(&path, &bytes)?;
    Ok(path)
}

fn create_dir(path: &Path) -> Result<(), Failure> {
    std::fs::create_dir_all(path).map_err(|error| {
        Failure::failed(
            "output_unwritable",
            format!("cannot create `{}`", path.display()),
        )
        .remedy("check the path and its permissions")
        .detail(json!({ "detail": error.kind().to_string() }))
    })
}

fn write_bytes(path: &Path, bytes: &[u8]) -> Result<(), Failure> {
    std::fs::write(path, bytes).map_err(|error| {
        Failure::failed(
            "output_unwritable",
            format!("cannot write `{}`", path.display()),
        )
        .remedy("check the path and its permissions")
        .detail(json!({ "detail": error.kind().to_string() }))
    })
}

fn sha256(bytes: &[u8]) -> String {
    format!("sha256:{:x}", Sha256::digest(bytes))
}

/// The owner's bounded answer plus what this host wrote.
fn project(answer: Value, written: Vec<Value>, report_path: &Path) -> Value {
    let total = written.len();
    let mut listed = written;
    let withheld = listed.len().saturating_sub(WRITTEN_LIMIT);
    listed.truncate(WRITTEN_LIMIT);
    let mut answer = match answer {
        Value::Object(map) => map,
        other => {
            let mut map = serde_json::Map::new();
            map.insert("answer".into(), other);
            map
        }
    };
    answer.insert("written".into(), json!(listed));
    answer.insert("written_count".into(), json!(total));
    answer.insert("report".into(), json!(report_path.to_string_lossy()));
    if withheld > 0 {
        answer.insert(
            "more".into(),
            json!({
                "written_withheld": withheld,
                "next": "read the exchange report for every artifact",
            }),
        );
    }
    Value::Object(answer)
}

pub fn render(data: &Value) -> String {
    let mut out = format!(
        "{}  plan {}\n  {} artifact(s) → {}\n",
        data["status"].as_str().unwrap_or(""),
        data["plan_id"].as_str().unwrap_or(""),
        data["written_count"],
        data["report"].as_str().unwrap_or(""),
    );

    if let Some(rows) = data["written"].as_array().filter(|rows| !rows.is_empty()) {
        out.push('\n');
        for row in rows {
            out.push_str(&format!(
                "  {:<44} {:>10} B  {}\n",
                row["path"].as_str().unwrap_or(""),
                row["byte_len"],
                row["sha256"].as_str().unwrap_or(""),
            ));
        }
    }

    if let Some(withheld) = data["more"]["written_withheld"].as_u64().filter(|n| *n > 0) {
        out.push_str(&format!("  … {withheld} more artifact(s)\n"));
    }

    render::list(&mut out, "LOSSES", &data["losses"]);
    render::list(&mut out, "WARNINGS", &data["warnings"]);
    out
}

#[cfg(test)]
#[path = "convert_tests.rs"]
mod tests;
