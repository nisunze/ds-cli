//! `ds map design version play` — open one immutable read-only snapshot.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Chapter, Command, Effect, Example, Execution, Refusal,
};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Value, json};

use super::TRANSFORMER_ARG;
use super::version_shared as shared;
use crate::DESCRIPTOR_ARG;

const VERSION_ARG: Arg = Arg::value(
    "version",
    "<version-id>",
    "Exact playable retained version.",
)
.required();

pub static COMMAND: Command = Command {
    id: "map.design.version.play",
    path: &["map", "design", "version", "play"],
    contract: 2,
    summary: "Open one retained transformer version read-only on the map.",
    purpose: "Asks the paired application to load one exact immutable transformer snapshot into its visible read-only playback context and waits for the map acknowledgement. No features cross the CLI. It never stages, restores, saves, or replaces working truth.",
    chapter: Chapter::Design,
    effect: Effect::LocalUi,
    authority: Authority::Project,
    execution: Execution::Sync,
    args: &[TRANSFORMER_ARG, VERSION_ARG, DESCRIPTOR_ARG],
    output: "A bounded playback receipt: project, transformer, exact version, version_playback context, whether it changed, map readiness, read_only=true, feature count, staged=false, and persisted=false.",
    examples: &[Example {
        command: "ds map design version play --transformer agasharu --version v2 --output json",
        note: "Opens the immutable v2 overlay; it cannot become an editable room.",
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
            when: "--version is not a canonical local-<digest> or v<number>",
            remedy: "list versions and pass one exact returned version id",
        },
        Refusal {
            code: "version_not_found",
            when: "the exact retained version does not exist",
            remedy: "list versions and pass an exact returned version_id",
        },
        Refusal {
            code: "playback_unavailable",
            when: "the version is a legacy metadata-only row",
            remedy: "choose a row whose playback_available is true",
        },
        Refusal {
            code: "dirty_room",
            when: "the active edit room has unsaved local work",
            remedy: "keep working there, or explicitly save or discard before playback",
        },
        Refusal {
            code: "project_mismatch",
            when: "the version and active project do not match",
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
    let version = shared::canonical_version(inputs.require("version")?, false)?;
    let descriptor = crate::paired(inputs.value("desktop-descriptor"))?;
    let result = crate::invoke(
        &descriptor,
        &crate::DESIGN_VERSION_PLAY,
        json!({ "transformer": transformer, "version": version }),
        crate::DESIGN_STAGE_TIMEOUT,
    )
    .map_err(shared::classify_failure)?;
    receipt(transformer, version, &result)
}

fn receipt(transformer: &str, version: &str, result: &Value) -> Result<Value, Failure> {
    let project = shared::nonempty_text(result, "project")?;
    let returned = shared::nonempty_text(result, "transformer")?;
    let version_id = shared::nonempty_text(result, "versionId")?;
    if returned != transformer || version_id != version {
        return Err(shared::unreadable(
            "the application acknowledged a different transformer or version",
        ));
    }
    if shared::nonempty_text(result, "contextType")? != "version_playback" {
        return Err(shared::unreadable(
            "the application did not enter the version_playback context",
        ));
    }
    shared::require_true(result, "mapReady")?;
    shared::require_true(result, "readOnly")?;
    shared::require_false(result, "staged")?;
    shared::require_false(result, "persisted")?;
    Ok(json!({
        "project": project,
        "transformer": returned,
        "version_id": version_id,
        "context_type": "version_playback",
        "context_changed": shared::boolean(result, "contextChanged")?,
        "map_ready": true,
        "read_only": true,
        "feature_count": shared::count(result, "featureCount")?,
        "staged": false,
        "persisted": false,
    }))
}

pub fn render(data: &Value) -> String {
    format!(
        "playing {}  ·  {}  ·  {} feature(s)  ·  read only{}\n",
        data["version_id"].as_str().unwrap_or("?"),
        data["transformer"].as_str().unwrap_or("?"),
        data["feature_count"].as_u64().unwrap_or(0),
        if data["context_changed"].as_bool().unwrap_or(false) {
            ""
        } else {
            " (already active)"
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn receipt_refuses_any_mutation_claim_and_drops_feature_payloads() {
        let raw = json!({
            "project":"p","transformer":"agasharu","versionId":"v2",
            "contextType":"version_playback","contextChanged":false,"mapReady":true,
            "readOnly":true,"featureCount":14,"staged":false,"persisted":false,
            "features":[{"must":"not cross"}]
        });
        let shaped = receipt("agasharu", "v2", &raw).expect("valid receipt");
        assert_eq!(shaped["read_only"], true);
        assert_eq!(shaped["context_changed"], false);
        assert_eq!(shaped["staged"], false);
        assert!(shaped.get("features").is_none());
        let mut unsafe_raw = raw;
        unsafe_raw["staged"] = json!(true);
        assert_eq!(
            receipt("agasharu", "v2", &unsafe_raw).unwrap_err().code(),
            "desktop_unreadable"
        );
    }
}
