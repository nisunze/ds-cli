//! `ds design collisions` — how many collisions this project has, headlessly.
//!
//! A collision means two or more transformers claim overlapping ground or a
//! clashing identity, and a combined report cannot be produced while one
//! stands. Until now no `ds` command modelled one — the crate's own test said
//! so, and asserted the word "collision" was ABSENT from a refusal, which
//! pinned the gap in place. An operator whose combined run was going to fail
//! had no way to be told why without opening the application.
//!
//! Detection itself is a governed report action the cloud reporter serves from
//! the project's own transformer data; this command READS the answer it wrote.
//! The count's five-candidate precedence and the states an operator reads are
//! `ds_command_kernel::collisions`', so the card and this command report the
//! same number under the same words.

/// The key an operator reads the answer under is the kernel's fold, not a
/// second `match` here: this command and the card name one condition one way.
use ds_command_kernel::collisions::state_key;

use ds_cli_auth::TransformerSet;
use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Chapter, Command, Effect, Example, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Value, json};

use super::transformer::LANE_ARG;

/// The reserved project-wide row the reporter writes collisions into. It is
/// never an LV transformer, which is why it is named here rather than taken.
const COLLISIONS_ROW: &str = "collisions";

pub static COMMAND: Command = Command {
    id: "design.collisions",
    path: &["design", "collisions"],
    contract: 1,
    summary: "Read collision counts and rank overlapping transformer regions.",
    purpose: "\
Reads the project-wide collisions document the report owner writes. The count \
is taken in a fixed precedence, because a run that finds NOTHING writes no \
layer: a project with no collisions reports zero, not unknown, and a malformed \
count stays unknown rather than becoming a number the owner never wrote. \
Detection is a separate governed action; this starts nothing and writes \
nothing. The reference names the precedence.",
    chapter: Chapter::Design,
    effect: Effect::LocalAuthState,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[
        crate::PROJECT_ARG,
        LANE_ARG,
        Arg::switch(
            "regions",
            "Read ranked region evidence from the saved collisions layer.",
        ),
        Arg::value(
            "limit",
            "<1-50>",
            "Maximum ranked regions to return; more reports truncation.",
        )
        .default("10"),
    ],
    output: "\
Lane and project identity, `checked`, `pairs` (null when unknown), the \
`state` key an operator reads it under, and how many ordinary transformers a \
detection run would cover. With --regions, ranked evidence without geometry, total and more.",
    examples: &[
        Example {
            command: "ds design collisions --output json",
            note: "`.data.pairs` is null when the project has never been checked.",
            runnable: false,
        },
        Example {
            command: "ds design collisions",
            note: "Read before `ds report project compounded`: a collision blocks it.",
            runnable: false,
        },
    ],
    refusals: &[
        super::transformer::NATIVE_PROFILE,
        super::transformer::NATIVE_PROFILE_DIGEST,
        super::transformer::NATIVE_PROFILE_UNSAFE,
        super::transformer::HEADLESS_SIGNED_OUT,
        super::transformer::PROJECT_REQUIRED,
        super::transformer::CONTEXT_CORRUPT,
        Refusal {
            code: "collision_limit_invalid",
            when: "limit is outside 1-50",
            remedy: "pass --limit 1-50",
        },
        Refusal {
            code: "collision_identity_changed",
            when: "the restored principal or lane changed between count and region reads",
            remedy: "retry the same explicit project under one connected account",
        },
        Refusal {
            code: "collision_regions_unreadable",
            when: "the saved collision layer exceeds its bounded projection",
            remedy: "refresh collision detection before reading regions",
        },
    ],
    reference: Some("docs/reference/design.md"),
    search: &["overlap", "severity", "ranking", "duplicate transformers"],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let limit: usize = inputs
        .require("limit")?
        .parse()
        .ok()
        .filter(|n| (1..=50).contains(n))
        .ok_or_else(|| {
            Failure::invalid("collision_limit_invalid", "limit must be 1-50")
                .remedy("pass --limit 1-50")
        })?;
    let requested = TransformerSet::new(std::iter::empty::<String>())
        .map_err(|error| Failure::invalid("invalid_transformer_scope", error.to_string()))?;
    let headless = ds_cli_auth::transformer_status(
        inputs.require("lane")?,
        inputs.require("project")?,
        &requested,
    )?;
    let mut output = super::transformer::project_receipt(&headless);
    let rows = headless.result().rows();
    let document = rows
        .iter()
        .find(|row| row.name() == COLLISIONS_ROW)
        .map(|row| row.row().clone());
    // Every ordinary transformer is in scope for a detection run; the reserved
    // project-wide rows are not transformers and are not checked.
    let checked = rows.iter().filter(|row| !is_reserved(row.name())).count();
    let pairs = document
        .as_ref()
        .and_then(|row| ds_command_kernel::collisions::project_count(row, &Value::Null));
    let object = output.as_object_mut().expect("receipt is an object");
    object.insert("checked".into(), json!(document.is_some()));
    object.insert("pairs".into(), json!(pairs));
    object.insert("state".into(), json!(state_key(document.is_some(), pairs)));
    object.insert("transformers".into(), json!(checked));
    if inputs.switch("regions") {
        let features = if document.is_some() && pairs != Some(0) {
            let context = ds_cli_auth::transformer_context_for_project(
                inputs.require("lane")?,
                inputs.require("project")?,
                COLLISIONS_ROW,
            )?;
            if context.identity() != headless.identity() {
                return Err(Failure::unauthorized(
                    "collision_identity_changed",
                    "collision read identity changed",
                )
                .remedy("retry the same explicit project under one connected account"));
            }
            output["region_source"] = json!({"version":context.snapshot().metadata().version(),"content_digest":context.snapshot().metadata().content_digest()});
            let layer = context.snapshot().layers().get("collisions");
            layer
                .and_then(|l| l["features"].as_array())
                .cloned()
                .ok_or_else(|| {
                    Failure::unavailable(
                        "collision_regions_unreadable",
                        "saved collision layer has no feature inventory",
                    )
                    .remedy("refresh collision detection before reading regions")
                })?
        } else {
            Vec::new()
        };
        if features.len() > 10_000
            || serde_json::to_vec(&features).map_or(true, |bytes| bytes.len() > 4 * 1024 * 1024)
        {
            return Err(Failure::unavailable(
                "collision_regions_unreadable",
                "collision evidence exceeds its bound",
            )
            .remedy("refresh collision detection before reading regions"));
        }
        output["summary"] = ds_command_kernel::collisions::summarize(
            &features,
            true,
            document.is_some(),
            None,
            limit,
        );
    }
    Ok(output)
}

/// The project-wide rows that are documents rather than transformers.
fn is_reserved(name: &str) -> bool {
    matches!(name, COLLISIONS_ROW | "combined_transformer" | "mv_data")
}

pub fn render(data: &Value) -> String {
    let pairs = data["pairs"]
        .as_u64()
        .map(|count| count.to_string())
        .unwrap_or_else(|| "unknown".into());
    let mut text = format!(
        "project {} · {} · {} pairs · {} transformers in scope · {}\n",
        super::transformer::project_label(data),
        data["lane"].as_str().unwrap_or("?"),
        pairs,
        data["transformers"].as_u64().unwrap_or(0),
        data["state"].as_str().unwrap_or("-"),
    );
    if let Some(regions) = data["summary"]["regions"].as_array() {
        for region in regions {
            let coverage = region["coveragePct"]
                .as_f64()
                .map(|n| format!("{n:.1}%"))
                .unwrap_or_else(|| "unmeasured".into());
            text.push_str(&format!(
                "  {} · coverage {} · LV {} · service {} · shared customers {}\n",
                region["id"].as_str().unwrap_or("?"),
                coverage,
                region["lvCrossings"],
                region["serviceCrossings"],
                region["sharedCustomers"]
            ));
        }
        if data["summary"]["more"] == true {
            text.push_str("  more regions omitted; increase --limit\n");
        }
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The keys are the kernel's; this pins that the command reads them from
    /// there rather than restating the fold.
    #[test]
    fn the_state_key_separates_never_checked_from_unknown_and_from_zero() {
        assert_eq!(state_key(false, None), "pctl_collisions_never_checked");
        assert_eq!(state_key(true, None), "pctl_collisions_unknown");
        assert_eq!(state_key(true, Some(0)), "pctl_collisions_pairs_none");
        assert_eq!(state_key(true, Some(1)), "pctl_collisions_pairs_one");
        assert_eq!(state_key(true, Some(7)), "pctl_collisions_pairs_many");
    }

    #[test]
    fn the_project_wide_documents_are_not_transformers_in_scope() {
        for name in ["collisions", "combined_transformer", "mv_data"] {
            assert!(is_reserved(name));
        }
        assert!(!is_reserved("TX-1"));
    }

    /// The precedence is the kernel's; this pins that the command reads the
    /// document through it rather than reaching for one member.
    #[test]
    fn a_zero_result_document_reports_zero_not_unknown() {
        let row = json!({
            "layers": {},
            "report_metadata": {"outcomes": {"collision_detection": {"details": {"pairs": 0}}}}
        });
        assert_eq!(
            ds_command_kernel::collisions::project_count(&row, &Value::Null),
            Some(0)
        );
    }
}
