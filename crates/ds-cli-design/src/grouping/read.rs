//! `ds design consumer-grouping read | archive` — the persisted plan itself.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{Authority, Chapter, Command, Effect, Example, Execution};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Value, json};

use crate::DESCRIPTOR_ARG;
use crate::grouping::PURPOSE_ARG;

pub static READ_COMMAND: Command = Command {
    id: "design.consumer-grouping.read",
    path: &["design", "consumer-grouping", "read"],
    contract: 1,
    summary: "Read the applied grouping plan for one purpose.",
    purpose: "\
Returns the stored plan exactly as it was applied — ordered definition ids, \
canonical tuple keys, membership, the projection digest, the plan digest, its \
revision, author and lifecycle — WITHOUT re-planning it. Seeing that a plan \
went stale is the reason to look at it, so this never refuses on staleness; a \
consumer about to publish reaches the plan through its own producer, which \
does.",
    chapter: Chapter::Design,
    effect: Effect::ReadOnly,
    authority: Authority::Project,
    execution: Execution::Sync,
    args: &[PURPOSE_ARG, DESCRIPTOR_ARG],
    output: "The stored plan: purpose, definition_ids, groups, counts, plan_digest, revision, lifecycle.",
    examples: &[Example {
        command: "ds design consumer-grouping read --purpose report_archive --output json",
        note: "This is the grouping a compounded archive files its folders by.",
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

pub static ARCHIVE_COMMAND: Command = Command {
    id: "design.consumer-grouping.archive",
    path: &["design", "consumer-grouping", "archive"],
    contract: 1,
    summary: "Retire the applied grouping plan for one purpose.",
    purpose: "\
Marks the plan archived. It is NOT deleted: the record of how an already \
published artifact was grouped has to survive, and what stops is its use as the \
current authority. A consumer that loads an archived plan refuses and asks for a \
new one to be applied.",
    chapter: Chapter::Design,
    effect: Effect::GlobalWrite,
    authority: Authority::Project,
    execution: Execution::Sync,
    args: &[PURPOSE_ARG, DESCRIPTOR_ARG],
    output: "The archived plan, with its lifecycle now `archived`.",
    examples: &[Example {
        command: "ds design consumer-grouping archive --purpose report_archive",
        note: "Archiving stops the plan being used; it never removes the record.",
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

pub fn run_read(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
    let descriptor = crate::paired(inputs.value("desktop-descriptor"))?;
    crate::invoke(
        &descriptor,
        &crate::CONSUMER_GROUPING_READ,
        json!({"purpose": crate::grouping::purpose(inputs)?}),
        crate::READ_TIMEOUT,
    )
    .map_err(crate::classify_design_failure)
}

pub fn run_archive(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
    let descriptor = crate::paired(inputs.value("desktop-descriptor"))?;
    crate::invoke(
        &descriptor,
        &crate::CONSUMER_GROUPING_ARCHIVE,
        json!({"purpose": crate::grouping::purpose(inputs)?}),
        crate::WRITE_TIMEOUT,
    )
    .map_err(crate::classify_design_failure)
}

pub fn render(data: &Value) -> String {
    let definitions: Vec<&str> = data["definition_ids"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect();
    let mut out = format!(
        "{} grouping · {} · revision {} · [{}]\n",
        data["purpose"].as_str().unwrap_or("?"),
        data["lifecycle"].as_str().unwrap_or("active"),
        data["revision"].as_u64().unwrap_or(0),
        // The ORDER is the identity: [city, phase] is a different plan from
        // [phase, city], so it is printed as authored and never sorted.
        if definitions.is_empty() {
            "one untagged group".to_string()
        } else {
            definitions.join(", ")
        },
    );
    out.push_str(&format!(
        "  plan {} · projection {}\n",
        data["plan_digest"].as_str().unwrap_or("?"),
        data["projection_sha256"].as_str().unwrap_or("?"),
    ));
    out.push_str(&format!(
        "  {} members, {} unassigned, {} groups\n",
        data["member_count"].as_u64().unwrap_or(0),
        data["unassigned_count"].as_u64().unwrap_or(0),
        data["groups"].as_array().map(Vec::len).unwrap_or(0),
    ));
    for group in data["groups"].as_array().into_iter().flatten() {
        let labels: Vec<String> = group["values"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|value| {
                let parts: Vec<&str> = value["labels"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str)
                    .collect();
                if parts.is_empty() {
                    "—".to_string()
                } else {
                    parts.join("+")
                }
            })
            .collect();
        out.push_str(&format!(
            "  {} · {} transformers{}\n",
            if labels.is_empty() {
                "untagged".to_string()
            } else {
                labels.join(" / ")
            },
            group["transformers"].as_array().map(Vec::len).unwrap_or(0),
            match group["source_city_id"].as_str() {
                Some(city) if !city.is_empty() => format!(" → solar {city}"),
                _ => String::new(),
            },
        ));
    }
    out
}
