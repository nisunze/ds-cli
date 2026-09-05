//! `ds map design version list` — bounded retained history metadata.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{Authority, Chapter, Command, Effect, Example, Execution, Refusal};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Value, json};

use super::TRANSFORMER_ARG;
use super::version_shared as shared;
use crate::DESCRIPTOR_ARG;

pub static COMMAND: Command = Command {
    id: "map.design.version.list",
    path: &["map", "design", "version", "list"],
    contract: 2,
    summary: "List a transformer's retained versions and playback availability.",
    purpose: "Lists bounded immutable version metadata for one exact transformer, newest first. Every row explicitly reports whether a playback snapshot exists. It never loads snapshot layers, opens a room, restores content, or changes the project.",
    chapter: Chapter::Design,
    effect: Effect::ReadOnly,
    authority: Authority::Project,
    execution: Execution::Sync,
    args: &[TRANSFORMER_ARG, DESCRIPTOR_ARG],
    output: "Project, transformer, total, truncation, and at most 200 metadata rows with version id, ordinal, reason, actor, time, and explicit playback_available.",
    examples: &[Example {
        command: "ds map design version list --transformer agasharu --output json",
        note: "Choose only a row whose playback_available is true for play or compare.",
        runnable: false,
    }],
    refusals: &[
        crate::NOT_PAIRED,
        crate::AMBIGUOUS,
        crate::UNREACHABLE,
        crate::PAIRING_REJECTED,
        super::DESIGN_REFUSED,
        Refusal {
            code: "transformer_not_found",
            when: "the exact transformer does not exist in the active project",
            remedy: "run `ds map design list --output json` and pass one exact transformer name",
        },
        Refusal {
            code: "project_mismatch",
            when: "the transformer and active project do not match",
            remedy: "open the intended project, verify desktop status, and retry",
        },
        crate::UNSUPPORTED,
        crate::UNREADABLE,
        crate::SIGNED_OUT,
    ],
    reference: Some("docs/reference/map.md"),
    availability: crate::paired_availability,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let transformer = inputs.require("transformer")?;
    let descriptor = crate::paired(inputs.value("desktop-descriptor"))?;
    let result = crate::invoke(
        &descriptor,
        &crate::DESIGN_VERSION_LIST,
        shared::one_argument("transformer", transformer),
        crate::DESIGN_READ_TIMEOUT,
    )
    .map_err(shared::classify_failure)?;
    receipt(transformer, &result)
}

fn receipt(transformer: &str, result: &Value) -> Result<Value, Failure> {
    let project = shared::nonempty_text(result, "project")?;
    let returned = shared::nonempty_text(result, "transformer")?;
    if returned != transformer {
        return Err(shared::unreadable(
            "the application returned version history for another transformer",
        ));
    }
    let rows = result["versions"]
        .as_array()
        .ok_or_else(|| shared::unreadable("the application omitted bounded version rows"))?;
    if rows.len() > shared::MAX_VERSION_ROWS {
        return Err(shared::unreadable(format!(
            "the application returned more than {} version rows",
            shared::MAX_VERSION_ROWS
        )));
    }
    let versions = rows
        .iter()
        .map(shared::version_row)
        .collect::<Result<Vec<_>, _>>()?;
    let total = shared::count(result, "total")?;
    if total < versions.len() as u64 {
        return Err(shared::unreadable(
            "the version total is smaller than the returned bounded rows",
        ));
    }
    Ok(json!({
        "project": project,
        "transformer": returned,
        "total": total,
        "truncated": shared::boolean(result, "truncated")?,
        "versions": versions,
    }))
}

pub fn render(data: &Value) -> String {
    let mut out = format!(
        "{} retained version(s) for {}{}\n",
        data["total"].as_u64().unwrap_or(0),
        data["transformer"].as_str().unwrap_or("?"),
        if data["truncated"].as_bool().unwrap_or(false) {
            " (bounded)"
        } else {
            ""
        },
    );
    if let Some(rows) = data["versions"].as_array() {
        for row in rows {
            out.push_str(&format!(
                "  {}  {}  {}\n",
                row["version_id"].as_str().unwrap_or("?"),
                if row["playback_available"].as_bool().unwrap_or(false) {
                    "playable"
                } else {
                    "metadata only"
                },
                row["reason"].as_str().unwrap_or(""),
            ));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn receipt_is_bounded_and_keeps_false_playback_explicit() {
        let raw = json!({
            "project": "project-a",
            "transformer": "agasharu",
            "total": 2,
            "truncated": false,
            "versions": [
                {"versionId":"v2","version":2,"reason":"review","createdAt":"2026-08-31T10:00:00Z","createdBy":"a@example.com","playbackAvailable":true,"snapshot":{"must":"not cross"}},
                {"versionId":"v1","version":1,"reason":null,"createdAt":null,"createdBy":null,"playbackAvailable":false}
            ]
        });
        let shaped = receipt("agasharu", &raw).expect("valid receipt");
        assert_eq!(shaped["versions"][1]["playback_available"], false);
        assert_eq!(shaped["versions"][1]["reason"], Value::Null);
        assert!(shaped["versions"][0].get("snapshot").is_none());
    }
}
