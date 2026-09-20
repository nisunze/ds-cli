//! `ds design consumer-grouping apply` — commit the plan that was previewed.

use crate::LANE_ARG;
use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{Authority, Chapter, Command, Effect, Example, Execution, Requires};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Value, json};

pub static COMMAND: Command = Command {
    id: "design.consumer-grouping.apply",
    path: &["design", "consumer-grouping", "apply"],
    contract: 1,
    summary: "Apply a previewed typed consumer grouping plan.",
    purpose: "\
Commits the plan `ds design consumer-grouping preview` returned, fenced by its \
plan digest: stale definitions, assignments or Solar inventory refuse rather \
than landing against a state nobody previewed. The purpose says which consumer \
the plan binds — `solar_report` to governed Solar cities, `report_archive` to \
the folder and section authority a compounded archive files by.",
    chapter: Chapter::Design,
    effect: Effect::GlobalWrite,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[
        crate::grouping::PURPOSE_ARG,
        crate::group::PROJECTION_TRANSFORMERS_ARG,
        crate::grouping::DEFINITION_IDS_ARG,
        crate::grouping::PLAN_DIGEST_ARG,
        LANE_ARG,
    ],
    output: "The stored record as applied — purpose, ordered definition ids, lifecycle, revision, plan_digest, projection_sha256 and member/unassigned/group counts. A refusal returns no plan at all.",
    examples: &[
        Example {
            command: "ds design consumer-grouping apply --purpose report_archive --transformers kigali_a,kigali_b --definition-ids city --digest <plan-digest> --yes",
            note: "The digest comes from `ds design consumer-grouping preview`; without --yes dispatch refuses first.",
            runnable: false,
        },
        Example {
            command: "ds design consumer-grouping read --purpose report_archive --output json",
            note: "The confirmation door: this family's only way to re-read what was applied.",
            runnable: false,
        },
    ],
    refusals: &crate::headless_refusals!(
        crate::NOT_PERMITTED,
        crate::READ_ONLY,
        crate::CONFLICT,
        crate::INVALID_VALUE_LIST,
        crate::TOO_MANY,
        crate::CONFIRMATION_REQUIRED,
    ),
    reference: Some("docs/reference/design.md"),
    search: &[],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};

pub fn run(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
    // Bounded before pairing, as on `preview`: a selection the caller got
    // wrong must read as their own refusal, not as a pairing state.
    let arguments = json!({
        "purpose": crate::grouping::purpose(inputs)?,
        "transformers": crate::group::projection_transformers(inputs)?,
        "definition-ids": crate::grouping::definition_ids(inputs)?,
        "bindings": inputs.value("bindings").unwrap_or("[]"),
        "digest": inputs.require("digest")?,
    });
    crate::headless::perform(
        "design.consumer-grouping.apply",
        arguments,
        inputs.value("lane").unwrap_or("stable"),
    )
}

/// The applied record, rendered exactly as `read` renders it.
///
/// The server returns the whole stored plan on apply, so the write confirms
/// itself from its own answer — a digest echo alone proves only that the
/// request was well-formed. `archive`, the sibling write, already renders this
/// way.
pub fn render(data: &Value) -> String {
    crate::grouping::read::render(data)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_digest_flag_names_the_producer_that_can_mint_it() {
        let digest = COMMAND
            .args
            .iter()
            .find(|arg| arg.name == "digest")
            .expect("apply fences on a digest");
        assert!(
            digest.summary.contains("consumer-grouping preview"),
            "the digest summary sends the operator to `{}`",
            digest.summary
        );
        assert!(
            !digest.summary.contains("design group preview"),
            "`design group preview` mints assignment-batch digests this command cannot accept"
        );
    }

    #[test]
    fn the_receipt_is_the_stored_record_not_a_digest_echo() {
        // What the server returns on apply is the whole stored plan, so the
        // write answers "what landed?" itself. Echoing the digest back proved
        // only that the request was well-formed, and the family's `read` is
        // the only other door.
        let applied = json!({
            "purpose": "report_archive",
            "lifecycle": "active",
            "revision": 4,
            "definition_ids": ["city", "phase"],
            "plan_digest": "sha256:plan",
            "projection_sha256": "sha256:projection",
            "member_count": 12,
            "unassigned_count": 1,
            "groups": [],
        });
        let receipt = render(&applied);
        assert_eq!(
            receipt,
            crate::grouping::read::render(&applied),
            "the applied record must read exactly as the stored one"
        );
        for proof in ["revision 4", "city, phase", "sha256:plan", "12 members"] {
            assert!(
                receipt.contains(proof),
                "the receipt does not carry `{proof}`:\n{receipt}"
            );
        }
    }

    #[test]
    fn the_write_names_the_read_that_confirms_it() {
        assert!(
            COMMAND
                .examples
                .iter()
                .any(|example| example.command.contains("consumer-grouping read")),
            "a governed write whose verification door is named nowhere leaves \
             re-running the write as the only way to look"
        );
    }

    #[test]
    fn a_consumer_grouping_has_no_untagged_only_plan() {
        let ids = COMMAND
            .args
            .iter()
            .find(|arg| arg.name == "definition-ids")
            .expect("apply groups by declared definitions");
        assert!(
            ids.required,
            "the server requires 1-16 definitions; omitting the flag must refuse at the parser"
        );
        assert!(
            !ids.summary.contains("untagged group"),
            "the projection's empty-selection promise is not this command's"
        );
    }
}
