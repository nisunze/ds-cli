//! `ds pls pole-capacity read` — read one structure's capacity block.
//!
//! The task returns a paged result and already carries `total_items`,
//! `offset`, `returned` and `next_offset`. `ds` does not re-paginate it: the
//! task's own pagination is the contract, and re-deriving it here would be a
//! second answer to a question the owner already answers.
//!
//! What `ds` adds is the continuation in the shape every other `ds` command
//! uses, so a caller who has learned to read `more` on one command reads it
//! the same way here.

use std::path::{Path, PathBuf};

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Availability, Command, Effect, Example, Execution, Refusal,
};
use ds_cli_contract::{Context, Inputs};
use ds_grid_tasks::pole_capacity::MAX_DESCRIBE_LIMIT;
use ds_grid_tasks::{DescribePoleCapacityRequest, describe_pole_capacity};
use serde_json::{Value, json};

use crate::{numeric, source_path, task_failure};

pub static COMMAND: Command = Command {
    id: "pls.pole-capacity.read",
    path: &["pls", "pole-capacity", "read"],
    contract: 1,
    summary: "Read a structure's pole capacity block, digest-pinned and paged.",
    purpose: "\
Reads the allowable-span capacity table out of a PLS-POLE analytical structure \
file and returns it with the source's exact digest, its declared units, and \
the provenance the engine recorded. The result is paged: it reports the total, \
what it returned, and the next offset, so a large block is walked rather than \
inlined.",
    effect: Effect::Discovery,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[
        Arg::value(
            "structure",
            "<path>",
            "The analytical structure file (.012).",
        )
        .required(),
        Arg::value("offset", "<n>", "Start at this item.").default("0"),
        Arg::value(
            "limit",
            "<n>",
            "Return at most this many items; the task bounds this at 64.",
        ),
    ],
    output: "\
The source's leaf, SHA-256 and byte length; the declared unit system; the \
capacity items; and `more.next_offset` when the block continues.",
    examples: &[Example {
        command: "ds pls pole-capacity read --structure ./structures/pole.012 --limit 5 --output json",
        note: "Digest is in .data.source_sha256; pin it when recording a result.",
        runnable: false,
    }],
    refusals: &[
        Refusal {
            code: "source_not_found",
            when: "--structure does not name a file",
            remedy: "check the path; it takes a .012 analytical structure",
        },
        Refusal {
            code: "invalid_number",
            when: "--offset is not a whole number, or --limit is outside the task's bound of 64",
            remedy: "the refusal carries the accepted range; offset starts at 0",
        },
        Refusal {
            code: "task_refused",
            when: "the task ran and refused — an unreadable file, or a limit outside its bound",
            remedy: "read detail.code and detail.detail for the task's own reason",
        },
        crate::RESULT_ENCODING_REFUSAL,
    ],
    reference: Some("docs/reference/pls.md"),
    availability: available,
};

fn available() -> Availability {
    Availability::Available
}

/// The task's own maximum, referenced rather than copied. `MAX_DESCRIBE_LIMIT`
/// is public on the owning module precisely so a host does not restate it.
fn capacity_limit(raw: Option<&str>) -> Result<usize, Failure> {
    let requested = numeric(raw, 20)?;
    if requested == 0 || requested > MAX_DESCRIBE_LIMIT {
        return Err(
            Failure::invalid("invalid_number", "--limit is outside the task's bound")
                .remedy(format!("pass 1..{MAX_DESCRIBE_LIMIT}"))
                .detail(json!({ "given": requested, "min": 1, "max": MAX_DESCRIBE_LIMIT })),
        );
    }
    Ok(requested)
}

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let raw = inputs.require("structure")?;
    let source: PathBuf = source_path(raw, "structure")?;

    let request = DescribePoleCapacityRequest {
        source_path: source,
        offset: numeric(inputs.value("offset"), 0)?,
        limit: capacity_limit(inputs.value("limit"))?,
    };

    let result = describe_pole_capacity(&request)
        .map_err(|error| task_failure(&error.code, &error.detail))?;

    let mut answer = serde_json::to_value(&result).map_err(|error| {
        Failure::internal(
            "result_unserializable",
            "the task result could not be encoded",
        )
        .detail(json!({ "detail": error.to_string() }))
    })?;

    // Restate the task's own continuation in the shape every `ds` command
    // uses. The authoritative numbers stay where the task put them.
    if let (Some(next), Some(object)) = (result.next_offset, answer.as_object_mut()) {
        object.insert(
            "more".into(),
            json!({
                "next_offset": next,
                "remaining": result.total_items.saturating_sub(request.offset + result.returned),
                "next": format!(
                    "ds pls pole-capacity read --structure {} --offset {next}",
                    Path::new(raw).display()
                ),
            }),
        );
    }

    Ok(answer)
}

pub fn render(data: &Value) -> String {
    let mut out = format!(
        "{}\n  {}\n  {} bytes · units {} · angle {} · span {}\n\n{} item(s) of {} from offset {}\n",
        data["source_leaf"].as_str().unwrap_or(""),
        data["source_sha256"].as_str().unwrap_or(""),
        data["source_bytes"],
        data["declared_units"].as_str().unwrap_or("?"),
        data["angle_unit"].as_str().unwrap_or("?"),
        data["span_unit"].as_str().unwrap_or("?"),
        data["returned"],
        data["total_items"],
        data["offset"],
    );
    for item in data["items"].as_array().into_iter().flatten() {
        out.push_str(&format!("  {item}\n"));
    }
    if let Some(next) = data["more"]["next"].as_str() {
        out.push_str(&format!("\nnext: {next}\n"));
    }
    out
}
