//! `ds design tag enrich` — governed administrative location as system tags.
//!
//! ```text
//!   enrich-preview → enrich-apply
//! ```
//!
//! This does not RESOLVE anything. The governed location path has already put
//! a transformer's administrative bounds on its record; this materializes that
//! evidence as system-managed tags so a report, a status filter, a CLI listing
//! and a GIS projection all group through the same definition/assignment model
//! an operator's own `phase` uses.
//!
//! Three properties a caller must not work around:
//!
//! * **Preview first.** `apply` carries the digest `preview` returned. The
//!   server recomputes it over the jurisdiction, the reference revision, every
//!   fenced definition version and every ordered outcome, so an enrichment
//!   approved against one state cannot land against another.
//! * **A repeat writes nothing.** Every outcome comes back `unchanged` and the
//!   apply opens no transaction at all. Re-running is the repair path, not a
//!   churn risk.
//! * **Nothing is invented.** A transformer the location path could not resolve
//!   is `not_located` and its stored values are kept and reported, not deleted.
//!   A project whose country has no governed source is
//!   `unsupported_jurisdiction`, which is a complete answer: that project groups
//!   by its own authored `city` or `region` definition instead.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, ArgKind, Authority, Chapter, Command, Effect, Example, Execution,
};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Value, json};

use crate::DESCRIPTOR_ARG;

/// ds-brain's own bound: one apply is one Firestore transaction, and 50
/// transformers × six administrative levels leaves room for the definition
/// upserts and the audit row inside one 500-write commit. A larger project is
/// enriched in named parts, each with its own digest.
pub const MAX_ENRICHMENT_TRANSFORMERS: usize = 50;

pub const TRANSFORMERS_ARG: Arg = Arg {
    name: "transformers",
    kind: ArgKind::Value,
    value: "<names>",
    required: true,
    default: None,
    choices: &[],
    summary: "Comma-separated transformer names this enrichment covers (1-50).",
};

pub const REFERENCE_REVISION_ARG: Arg = Arg {
    name: "reference-revision",
    kind: ArgKind::Value,
    value: "<revision>",
    required: false,
    default: None,
    choices: &[],
    summary: "Pin the reference revision the evidence must have been resolved from.",
};

pub const DIGEST_ARG: Arg = Arg {
    name: "digest",
    kind: ArgKind::Value,
    value: "<plan-digest>",
    required: true,
    default: None,
    choices: &[],
    summary: "The digest `ds design tag enrich-preview` returned for this exact plan.",
};

pub fn transformers(inputs: &Inputs) -> Result<Vec<String>, Failure> {
    crate::list_values(
        inputs.require("transformers")?,
        "transformers",
        MAX_ENRICHMENT_TRANSFORMERS,
    )
}

fn arguments(inputs: &Inputs) -> Result<Value, Failure> {
    Ok(json!({
        "transformers": transformers(inputs)?,
        "reference-revision": inputs.value("reference-revision").unwrap_or_default(),
    }))
}

pub static PREVIEW_COMMAND: Command = Command {
    id: "design.tag.enrich-preview",
    path: &["design", "tag", "enrich-preview"],
    contract: 1,
    summary: "Plan the governed administrative tags for named transformers.",
    purpose: "\
Reads what the governed location path already resolved for each named \
transformer and plans the system-managed administrative tags that represent it. \
Writes nothing: this is the read an operator confirms before anything lands, \
and it keeps working on a project that accepts no changes. Every outcome is \
explicit — assign, reassign, unchanged, unassign, not_located, \
unsupported_jurisdiction or refused — and the plan digest fences the apply.",
    chapter: Chapter::Design,
    effect: Effect::ReadOnly,
    authority: Authority::Project,
    execution: Execution::Sync,
    args: &[TRANSFORMERS_ARG, REFERENCE_REVISION_ARG, DESCRIPTOR_ARG],
    output: "\
The project, resolved jurisdiction and country, the reference revision (or \
`unpinned`), the governed definitions the plan touches, one ordered outcome per \
transformer and level, per-action counts, and the plan digest.",
    examples: &[Example {
        command: "ds design tag enrich-preview --transformers kigali_a,kigali_b --output json",
        note: "Read counts and outcomes before applying; `unsupported_jurisdiction` is a valid answer.",
        runnable: false,
    }],
    refusals: &[
        crate::NOT_PAIRED,
        crate::AMBIGUOUS,
        crate::UNREACHABLE,
        crate::PAIRING_REJECTED,
        crate::DESIGN_REFUSED,
        crate::UNSUPPORTED,
        crate::UNREADABLE,
        crate::SIGNED_OUT,
        crate::NOT_PERMITTED,
    ],
    reference: Some("docs/reference/design.md"),
    availability: crate::paired_availability,
};

pub static APPLY_COMMAND: Command = Command {
    id: "design.tag.enrich-apply",
    path: &["design", "tag", "enrich-apply"],
    contract: 1,
    summary: "Apply exactly the previewed administrative tag plan.",
    purpose: "\
Commits the plan whose digest is passed, in one transaction: the governed \
definitions, every assignment write and delete, and the audit row land together \
or not at all. A digest that no longer describes the project refuses. Applying \
a plan that is already applied is a no-op that opens no transaction, so \
re-running this is the repair path. Source evidence — the raw administrative \
fields, their codes and the geometry behind them — is never touched.",
    chapter: Chapter::Design,
    effect: Effect::GlobalWrite,
    authority: Authority::Project,
    execution: Execution::Sync,
    args: &[
        TRANSFORMERS_ARG,
        DIGEST_ARG,
        REFERENCE_REVISION_ARG,
        DESCRIPTOR_ARG,
    ],
    output: "The same plan, with `applied` true and the counts that landed.",
    examples: &[Example {
        command: "ds design tag enrich-apply --transformers kigali_a --digest <plan-digest>",
        note: "The digest comes from preview; ds carries it and never mints one.",
        runnable: false,
    }],
    refusals: &[
        crate::NOT_PAIRED,
        crate::AMBIGUOUS,
        crate::UNREACHABLE,
        crate::PAIRING_REJECTED,
        crate::DESIGN_REFUSED,
        crate::UNSUPPORTED,
        crate::UNREADABLE,
        crate::SIGNED_OUT,
        crate::NOT_PERMITTED,
        crate::group::PLAN_STALE,
    ],
    reference: Some("docs/reference/design.md"),
    availability: crate::paired_availability,
};

// The arguments are built BEFORE the descriptor is resolved, in both doors.
// An over-large transformer set is this command's own refusal with its own
// remedy; discovering it only after failing to find a paired desktop would
// report the wrong problem on a machine that simply has no desktop running.

pub fn run_preview(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let arguments = arguments(inputs)?;
    let descriptor = crate::paired(inputs.value("desktop-descriptor"))?;
    crate::invoke(
        &descriptor,
        &crate::TAG_ENRICH_PREVIEW,
        arguments,
        crate::READ_TIMEOUT,
    )
    .map_err(crate::classify_design_failure)
}

pub fn run_apply(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let mut arguments = arguments(inputs)?;
    arguments["digest"] = Value::String(inputs.require("digest")?.to_string());
    let descriptor = crate::paired(inputs.value("desktop-descriptor"))?;
    crate::invoke(
        &descriptor,
        &crate::TAG_ENRICH_APPLY,
        arguments,
        crate::WRITE_TIMEOUT,
    )
    .map_err(crate::classify_design_failure)
}

pub fn render(data: &Value) -> String {
    let mut out = format!(
        "location enrichment · {} · {} · reference {}\n",
        data["jurisdiction"].as_str().unwrap_or("unsupported"),
        if data["applied"].as_bool().unwrap_or(false) {
            "applied"
        } else {
            "preview"
        },
        data["referenceRevision"].as_str().unwrap_or("?"),
    );
    if let Some(digest) = data["digest"].as_str() {
        out.push_str(&format!("  plan {digest}\n"));
    }
    // Counts first: an operator's decision is usually "how many change?", and
    // a fifty-row outcome list buries that.
    if let Some(counts) = data["counts"].as_object() {
        let mut ordered: Vec<(&String, &Value)> = counts.iter().collect();
        ordered.sort_by_key(|(action, _)| action.as_str());
        let summary: Vec<String> = ordered
            .iter()
            .map(|(action, count)| format!("{} {}", count.as_u64().unwrap_or(0), action))
            .collect();
        if !summary.is_empty() {
            out.push_str(&format!("  {}\n", summary.join(", ")));
        }
    }
    for outcome in data["outcomes"].as_array().into_iter().flatten() {
        let mut line = format!(
            "  {} {}",
            outcome["transformer"].as_str().unwrap_or("?"),
            outcome["action"].as_str().unwrap_or("?"),
        );
        if let Some(definition) = outcome["definition_id"].as_str() {
            line.push_str(&format!(" {definition}"));
        }
        if let Some(value) = outcome["value"].as_str() {
            line.push_str(&format!(" = {value}"));
        }
        if let Some(previous) = outcome["previous"].as_str() {
            line.push_str(&format!(" (was {previous})"));
        }
        if let Some(detail) = outcome["detail"].as_str() {
            line.push_str(&format!(" — {detail}"));
        }
        out.push_str(&line);
        out.push('\n');
    }
    out
}
