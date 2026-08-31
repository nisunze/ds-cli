//! `ds map design version compare` — open one bounded read-only comparison.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Chapter, Command, Effect, Example, Execution, Refusal,
};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Value, json};

use super::TRANSFORMER_ARG;
use super::version_shared as shared;
use crate::DESCRIPTOR_ARG;

const FROM_ARG: Arg = Arg::value("from", "<vN>", "Pinned retained version on the left.").required();
const TO_ARG: Arg = Arg::value(
    "to",
    "<vN|head>",
    "Pinned retained version, or current saved head, on the right.",
)
.required();

pub static COMMAND: Command = Command {
    id: "map.design.version.compare",
    path: &["map", "design", "version", "compare"],
    contract: 1,
    summary: "Compare one retained transformer version with another or saved head.",
    purpose: "Asks the paired application and its deterministic kernel to open a visible bounded comparison between one pinned retained version and another retained version or the current saved head. Only descriptors and aggregate counts cross the CLI; no features, restore payload, or edit is transported.",
    chapter: Chapter::Design,
    effect: Effect::LocalUi,
    authority: Authority::Project,
    execution: Execution::Sync,
    args: &[TRANSFORMER_ARG, FROM_ARG, TO_ARG, DESCRIPTOR_ARG],
    output: "Project and transformer; pinned left/right descriptors; observation time; exact aggregate and per-layer change counts; truncation; dialog readiness; staged=false and persisted=false.",
    examples: &[Example {
        command: "ds map design version compare --transformer agasharu --from v1 --to head --output json",
        note: "Pins v1 against the observed saved-head generation and opens the read-only comparison dialog.",
        runnable: false,
    }],
    refusals: &[
        crate::NOT_PAIRED,
        crate::AMBIGUOUS,
        crate::UNREACHABLE,
        crate::PAIRING_REJECTED,
        crate::SIGNED_OUT,
        super::DESIGN_REFUSED,
        Refusal {
            code: "transformer_not_found",
            when: "the exact transformer does not exist in the active project",
            remedy: "run `ds map design list --output json` and pass one exact transformer name",
        },
        Refusal {
            code: "invalid_version",
            when: "--from is not v<number>, or --to is neither v<number> nor head",
            remedy: "list versions, then pass --from vN --to vM|head",
        },
        Refusal {
            code: "invalid_comparison",
            when: "--from and --to name the same retained version",
            remedy: "choose two different versions, or compare the version with head",
        },
        Refusal {
            code: "version_not_found",
            when: "a pinned retained version does not exist",
            remedy: "list versions and pass exact returned version ids",
        },
        Refusal {
            code: "playback_unavailable",
            when: "a retained side is a legacy metadata-only row",
            remedy: "choose rows whose playback_available is true",
        },
        Refusal {
            code: "project_mismatch",
            when: "the transformer comparison and active project do not match",
            remedy: "open the intended project, verify desktop status, and retry",
        },
        crate::UNSUPPORTED,
        crate::UNREADABLE,
    ],
    reference: Some("docs/reference/map.md"),
    availability: crate::paired_availability,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let transformer = inputs.require("transformer")?;
    let from = shared::canonical_version(inputs.require("from")?, false)?;
    let to = shared::canonical_version(inputs.require("to")?, true)?;
    validate_distinct_sides(from, to)?;
    let descriptor = crate::paired(inputs.value("desktop-descriptor"))?;
    let result = crate::invoke(
        &descriptor,
        &crate::DESIGN_VERSION_COMPARE,
        json!({ "transformer": transformer, "from": from, "to": to }),
        crate::DESIGN_STAGE_TIMEOUT,
    )
    .map_err(shared::classify_failure)?;
    receipt(transformer, from, to, &result)
}

fn validate_distinct_sides(from: &str, to: &str) -> Result<(), Failure> {
    if from == to {
        return Err(Failure::invalid(
            "invalid_comparison",
            "--from and --to must not name the same retained version",
        )
        .remedy("choose two different versions, or compare the version with head"));
    }
    Ok(())
}

fn receipt(transformer: &str, from: &str, to: &str, result: &Value) -> Result<Value, Failure> {
    let project = shared::nonempty_text(result, "project")?;
    let returned = shared::nonempty_text(result, "transformer")?;
    if returned != transformer {
        return Err(shared::unreadable(
            "the application returned a comparison for another transformer",
        ));
    }
    let left = shared::descriptor(&result["from"], "from")?;
    let right = shared::descriptor(&result["to"], "to")?;
    if left["version_id"] != from
        || (to == "head" && right["kind"] != "saved_head")
        || (to != "head" && right["version_id"] != to)
    {
        return Err(shared::unreadable(
            "the comparison descriptors do not match the pinned request",
        ));
    }
    shared::require_true(result, "dialogReady")?;
    shared::require_false(result, "staged")?;
    shared::require_false(result, "persisted")?;
    let totals = shared::change_counts(&result["totals"])?;
    let layers = shared::comparison_layers(&result["layers"])?;
    Ok(json!({
        "project": project,
        "transformer": returned,
        "from": left,
        "to": right,
        "observed_at": shared::nonempty_text(result, "observedAt")?,
        "dialog_ready": true,
        "totals": totals,
        "layers": layers,
        "details_truncated": shared::boolean(result, "detailsTruncated")?,
        "staged": false,
        "persisted": false,
    }))
}

pub fn render(data: &Value) -> String {
    let right = if data["to"]["kind"] == "saved_head" {
        format!("head@{}", data["to"]["generation"].as_u64().unwrap_or(0))
    } else {
        data["to"]["version_id"].as_str().unwrap_or("?").to_string()
    };
    format!(
        "{}  {} -> {}  ·  {} layer(s)  ·  comparison ready\n",
        data["transformer"].as_str().unwrap_or("?"),
        data["from"]["version_id"].as_str().unwrap_or("?"),
        right,
        data["layers"].as_array().map_or(0, Vec::len),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn counts(seed: u64) -> Value {
        json!({
            "unchanged":seed,"localOnly":seed+1,"cloudOnly":seed+2,
            "attributeOnlyChanged":seed+3,"geometryOnlyChanged":seed+4,
            "attributeAndGeometryChanged":seed+5,"ambiguousUnmatchable":seed+6,
        })
    }

    #[test]
    fn receipt_pins_head_generation_and_keeps_only_bounded_counts() {
        let raw = json!({
            "project":"p","transformer":"agasharu",
            "from":{"kind":"version","versionId":"v1"},
            "to":{"kind":"saved_head","generation":8},
            "observedAt":"2026-08-31T12:00:00Z","dialogReady":true,
            "totals":counts(10),
            "layers":[{"layerName":"lv_lines","counts":counts(1),"features":[{"must":"not cross"}]}],
            "detailsTruncated":true,"staged":false,"persisted":false,
            "details":[{"must":"not cross"}]
        });
        let shaped = receipt("agasharu", "v1", "head", &raw).expect("valid receipt");
        assert_eq!(shaped["to"], json!({"kind":"saved_head","generation":8}));
        assert_eq!(shaped["layers"][0]["counts"]["local_only"], 2);
        assert!(shaped.get("details").is_none());
        assert!(shaped["layers"][0].get("features").is_none());
        assert_eq!(shaped["staged"], false);
        assert_eq!(shaped["persisted"], false);
    }

    #[test]
    fn identical_version_sides_are_refused_before_desktop_pairing() {
        let failure = validate_distinct_sides("v2", "v2").unwrap_err();
        assert_eq!(failure.code(), "invalid_comparison");
        assert!(
            failure
                .remedy_text()
                .unwrap_or_default()
                .contains("different")
        );
    }
}
