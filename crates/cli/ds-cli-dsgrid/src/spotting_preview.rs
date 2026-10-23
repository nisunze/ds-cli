//! `ds dsgrid spotting preview` — one exact-package, revision-pinned spotting
//! preview for review before an explicit apply.
//!
//! The installed Network addon plans the whole model or an exact ordered
//! alignment scope (`plan_whole_model_spotting`) and builds the one
//! `apply_spotting_layouts` application (`spotting_layout_application`) for
//! the same pinned package. Core pins the bytes and revision and decides the
//! reviewable status: any refused plan, refused request derivation, missing
//! fixed structure or refused application blocks it with typed conflicts and
//! no application. Nothing is journaled, saved or written to the model; the
//! Desktop applies a reviewed application through its Profile session.
use crate::{apply, native, package};
use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Availability, Chapter, Command, Effect, Example, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Inputs};
use ds_network_client::{CapturedNetworkClient, NetworkFailure};
use ds_network_contracts::grid::{
    NATIVE_GRID_QUERY_SCHEMA_V1, NativeGridQueryRequestV1, NativeGridQueryResultV1,
    NativeGridQuerySourcePin,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

pub const SCHEMA: &str = "ds.dsgrid.spotting-preview/v1";
const ARTIFACT_SCHEMA: &str = "ds.dsgrid.spotting-preview-artifact/v1";
const PLAN_OPERATION: &str = "plan_whole_model_spotting";
const APPLICATION_OPERATION: &str = "spotting_layout_application";
const MAX_ALIGNMENTS: usize = 4096;

const OWN_REFUSALS: &[Refusal] = &[
    Refusal {
        code: "spotting_preview_input_invalid",
        when: "the SHA-256, revision or an alignment id is empty, malformed or out of bounds",
        remedy: "pass the exact lowercase package SHA-256, its authored revision and nonempty alignment ids",
    },
    Refusal {
        code: "spotting_preview_digest_mismatch",
        when: "the package bytes differ from --expected-sha256",
        remedy: "capture the package again and preview its exact bytes",
    },
    Refusal {
        code: "output_exists",
        when: "--out already exists",
        remedy: "choose a new output path; this command never overwrites",
    },
    Refusal {
        code: "output_parent_missing",
        when: "the parent of --out does not exist",
        remedy: "create the intended output directory, then retry",
    },
    Refusal {
        code: "output_unwritable",
        when: "--out cannot be written completely",
        remedy: "check free space and permissions; a partial file is removed",
    },
];
const REFUSALS: &[Refusal] =
    &native::extend_refusals::<{ OWN_REFUSALS.len() + native::REFUSALS.len() }>(OWN_REFUSALS);

pub static COMMAND: Command = Command {
    id: "dsgrid.spotting.preview",
    path: &["dsgrid", "spotting", "preview"],
    contract: 1,
    summary: "Preview one revision-pinned spotting plan and its model application.",
    purpose: "\
Plans automatic spotting and stringing for the whole model or an exact ordered \
alignment scope on one digest- and revision-pinned .dsgrid package, then builds \
the single apply_spotting_layouts application for review. A refused plan, \
request derivation, missing fixed structure or refused application blocks the \
preview with typed conflicts and no application. Nothing is journaled, saved \
or published; apply the reviewed application in the model's edit session.",
    chapter: Chapter::GridModel,
    effect: Effect::LocalFileWrite,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[
        Arg::value("model", "<dsgrid-file>", "The exact .dsgrid package to plan.").required(),
        Arg::value(
            "expected-sha256",
            "<sha256>",
            "Lowercase SHA-256 of the exact package bytes.",
        )
        .required(),
        Arg::value(
            "expected-revision",
            "<revision>",
            "The authored revision the preview is pinned to.",
        )
        .required(),
        Arg::repeated(
            "alignment",
            "<id>",
            "One alignment of an exact ordered scope; omit for the whole model.",
        ),
        Arg::value(
            "out",
            "<absolute-new-json-file>",
            "Optional new absolute result file; returns a bounded receipt. Never replaces a file.",
        ),
    ],
    output: "\
ds.dsgrid.spotting-preview/v1: status ready|blocked, the pinned source (model, \
package and authored revision, SHA-256, size), the Network planning result in \
requested order, the application (null when blocked) and typed conflicts. With \
--out, ds.dsgrid.spotting-preview-artifact/v1 names the file, size and SHA-256.",
    examples: &[
        Example {
            command: "ds dsgrid spotting preview --model ./model.dsgrid --expected-sha256 <sha256> --expected-revision <rev> --output json",
            note: "Plan the whole model and review the application before applying it.",
            runnable: false,
        },
        Example {
            command: "ds dsgrid spotting preview --model ./model.dsgrid --expected-sha256 <sha256> --expected-revision <rev> --alignment al-branch --alignment al-main --out /abs/preview.json --output json",
            note: "Plan an exact ordered alignment scope into a new result file.",
            runnable: false,
        },
    ],
    refusals: REFUSALS,
    reference: Some("docs/reference/dsgrid.md"),
    search: &["automatic stringing", "spotting preview", "auto stringing"],
    requires: Requires::Server,
    availability: available,
};

fn available() -> Availability {
    ds_network_client::available()
}

fn input_invalid(message: impl Into<String>) -> Failure {
    Failure::invalid("spotting_preview_input_invalid", message.into())
        .remedy("pass the exact lowercase package SHA-256, its authored revision and nonempty alignment ids")
}

/// Admit the declared scalar inputs before any read or engine capture.
pub(crate) fn admit(
    expected_sha256: &str,
    expected_revision: &str,
    alignments: &[String],
) -> Result<(), Failure> {
    if expected_sha256.len() != 64
        || !expected_sha256
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(input_invalid("--expected-sha256 must be 64 lowercase hex digits"));
    }
    if expected_revision.is_empty() || expected_revision.len() > 200 {
        return Err(input_invalid("--expected-revision must name one authored revision"));
    }
    if alignments.len() > MAX_ALIGNMENTS
        || alignments
            .iter()
            .any(|id| id.is_empty() || id.len() > 512 || id.trim() != id)
    {
        return Err(input_invalid(
            "each --alignment must be one exact nonempty id (at most 4096)",
        ));
    }
    Ok(())
}

/// The typed conflicts that block a preview, from the planning result alone.
/// Absent optional collections are empty (Network omits empty ones).
pub(crate) fn planning_conflicts(result: &Value) -> Vec<Value> {
    let mut conflicts = Vec::new();
    let refused = result["batch"]["refused_requests"].as_u64().unwrap_or(0);
    if refused > 0 {
        conflicts.push(json!({
            "code": "alignment_plan_refused",
            "message": format!("{refused} alignment plan(s) were refused; no application can be offered."),
            "remedy": "Resolve the named alignment findings in the model or its spotting settings, then preview again.",
            "detail": result["batch"]["items"].clone(),
        }));
    }
    let derivations = result["refused_derivations"].as_array().map_or(0, Vec::len);
    if derivations > 0 {
        conflicts.push(json!({
            "code": "alignment_request_refused",
            "message": format!("{derivations} alignment request(s) could not be derived from the model settings."),
            "remedy": "Resolve the listed request derivation findings in the model or its spotting settings, then preview again.",
            "detail": result["refused_derivations"].clone(),
        }));
    }
    let missing = result["unresolved_fixed_structure_ids"]
        .as_array()
        .map_or(0, Vec::len);
    if missing > 0 {
        conflicts.push(json!({
            "code": "fixed_structure_missing",
            "message": format!("{missing} configured fixed structure(s) are absent from this model."),
            "remedy": "Update the fixed structure ids in the spotting settings, then preview again.",
            "detail": result["unresolved_fixed_structure_ids"].clone(),
        }));
    }
    conflicts
}

/// The planned alignments' exact plans, in requested order.
pub(crate) fn planned(result: &Value) -> Vec<Value> {
    result["batch"]["items"]
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter(|item| item["status"] == "planned")
                .map(|item| item["plan"].clone())
                .collect()
        })
        .unwrap_or_default()
}

/// A typed application refusal from the owner blocks the preview; any other
/// failure (missing/incompatible engine, staging) is the command's refusal.
pub(crate) fn application_conflict(error: &NetworkFailure) -> Option<Value> {
    (error.owner_code() == Some("operation_failed")).then(|| {
        json!({
            "code": "layout_application_refused",
            "message": error.to_string(),
            "remedy": "Resolve the typed conflict, then preview again. No model changes were made.",
            "detail": error.owner_detail.as_deref().cloned().unwrap_or(Value::Null),
        })
    })
}

pub(crate) fn assemble(
    query: &NativeGridQueryResultV1,
    sha256: &str,
    byte_len: usize,
    application: Option<Value>,
    conflicts: Vec<Value>,
) -> Value {
    let ready = application.is_some() && conflicts.is_empty();
    json!({
        "schema": SCHEMA,
        "status": if ready { "ready" } else { "blocked" },
        "source": {
            "model_id": query.source.model_id.as_str(),
            "package_revision": query.model_revision,
            "authored_revision": query.source.expected_revision,
            "package_sha256": sha256,
            "byte_len": byte_len,
        },
        "engine": query.engine_version,
        "result": query.result,
        "application": if ready { application.unwrap_or(Value::Null) } else { Value::Null },
        "conflicts": conflicts,
    })
}

fn query(
    client: &CapturedNetworkClient,
    bytes: &[u8],
    sha256: &str,
    revision: &str,
    operation: &str,
    params: Value,
    scratch: &std::path::Path,
) -> Result<NativeGridQueryResultV1, NetworkFailure> {
    let request = NativeGridQueryRequestV1 {
        schema: NATIVE_GRID_QUERY_SCHEMA_V1.into(),
        source: NativeGridQuerySourcePin {
            package_sha256: sha256.to_owned(),
            expected_revision: Some(revision.to_owned()),
            engineering_input_root: None,
        },
        operation_id: operation.into(),
        params,
    };
    // A stale --expected-revision is the owner's own typed stale_revision.
    client
        .grid_query(&request, bytes, scratch, &|| false)
        .map(|outcome| outcome.result)
}

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let path = inputs.require("model")?;
    let expected_sha256 = inputs.require("expected-sha256")?;
    let expected_revision = inputs.require("expected-revision")?;
    let alignments = inputs.repeated("alignment").to_vec();
    admit(expected_sha256, expected_revision, &alignments)?;
    let out = inputs.value("out");
    if let Some(out) = out {
        if !std::path::Path::new(out).is_absolute() {
            return Err(input_invalid("--out must be a new absolute file"));
        }
        apply::validate_output_path(out)?;
    }
    let bytes = package::read_bytes(path)?;
    let sha256 = format!("{:x}", Sha256::digest(&bytes));
    if sha256 != expected_sha256 {
        return Err(Failure::invalid(
            "spotting_preview_digest_mismatch",
            "the package bytes changed before planning",
        )
        .remedy("capture the package again and preview its exact bytes"));
    }
    let mut params = json!({});
    if !alignments.is_empty() {
        params["alignment_ids"] = json!(alignments);
    }
    // Execution memory admission is a host fact; Network owns the planning.
    if let Some(budget) = crate::host_memory::spotting_memory_budget_bytes() {
        params["memory_budget_bytes"] = json!(budget);
    }
    let client = native::capture()?;
    let plan_scratch = native::scratch()?;
    let plan = query(
        &client,
        &bytes,
        &sha256,
        expected_revision,
        PLAN_OPERATION,
        params,
        plan_scratch.path(),
    )
    .map_err(|error| native::at_path(native::map_failure(error), path))?;
    let mut conflicts = planning_conflicts(&plan.result);
    let mut application = None;
    if conflicts.is_empty() {
        let application_scratch = native::scratch()?;
        match query(
            &client,
            &bytes,
            &sha256,
            expected_revision,
            APPLICATION_OPERATION,
            json!({ "plans": planned(&plan.result) }),
            application_scratch.path(),
        ) {
            Ok(built) => application = Some(built.result),
            Err(error) => match application_conflict(&error) {
                Some(conflict) => conflicts.push(conflict),
                None => return Err(native::at_path(native::map_failure(error), path)),
            },
        }
    }
    let preview = assemble(&plan, &sha256, bytes.len(), application, conflicts);
    let Some(out) = out else {
        return Ok(preview);
    };
    let encoded = serde_json::to_vec(&preview).map_err(|error| {
        Failure::internal("output_unwritable", format!("cannot encode the preview: {error}"))
    })?;
    apply::write_new(out, &encoded)?;
    Ok(json!({
        "schema": ARTIFACT_SCHEMA,
        "out": out,
        "byte_count": encoded.len(),
        "sha256": format!("{:x}", Sha256::digest(&encoded)),
        "status": preview["status"],
        "source": preview["source"],
    }))
}

pub fn render(data: &Value) -> String {
    serde_json::to_string_pretty(data).unwrap_or_else(|_| data.to_string())
}

#[cfg(test)]
#[path = "spotting_preview_tests.rs"]
mod tests;
