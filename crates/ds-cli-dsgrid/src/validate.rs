//! `ds dsgrid validate` — is this package sound, and is the model inside it
//! sound?
//!
//! Those are two questions and this command answers them separately, because
//! they fail for different reasons and are fixed differently.
//!
//! **The container** is verified by decoding: every member is checked against
//! the manifest's byte length, digest, row count and schema fingerprint. A
//! failure here means the file is damaged or was written by an incompatible
//! release — the model inside is not even readable, so there is nothing to
//! say about it.
//!
//! **The model** is then validated by `ds-grid-model` itself: duplicate ids,
//! dangling references, and the rest of its reference rules. A failure here
//! means the file is intact and the authored content is wrong.
//!
//! Conflating the two would tell a caller "invalid" and leave them guessing
//! which kind of wrong it was.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Availability, Chapter, Command, Effect, Example, Execution, Refusal,
};
use ds_cli_contract::{Context, Inputs};
use ds_grid_model::validate_snapshot;
use serde_json::{Value, json};

use crate::package;

pub static COMMAND: Command = Command {
    id: "dsgrid.validate",
    path: &["dsgrid", "validate"],
    contract: 1,
    summary: "Verify a .dsgrid package and validate the model inside it.",
    purpose: "\
Answers two separate questions. First, does the container hold together — does \
every member match the byte length, digest, row count and schema fingerprint \
the manifest attests to? Second, is the authored model sound by its own rules \
— no duplicate ids, no dangling references? A package can pass the first and \
fail the second, and the two are fixed in completely different ways, so they \
are reported apart.",
    chapter: Chapter::GridModel,
    effect: Effect::Discovery,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[
        Arg::value("model", "<path>", "The .dsgrid package to verify.").required(),
        Arg::value("limit", "<n>", "Cap the listed issues.").default(package::DEFAULT_LIMIT),
    ],
    output: "\
`container` and `model`, each with its own verdict. Model issues carry a stable \
code, the table and entity concerned, and a message. `more.truncated` reports \
any issues withheld by --limit.",
    examples: &[Example {
        command: "ds dsgrid validate --model ./model.dsgrid --output json",
        note: "Exit 0 whether or not issues were found; read .data.model.valid.",
        runnable: false,
    }],
    refusals: &[
        Refusal {
            code: "model_not_found",
            when: "the path does not exist or is not a file",
            remedy: "check the path; --model takes a file, not a directory",
        },
        Refusal {
            code: "model_too_large",
            when: "the file is above the 512 MiB read bound",
            remedy: "confirm the file is a .dsgrid package and not a disk image",
        },
        Refusal {
            code: "model_unreadable",
            when: "the file exists but cannot be read",
            remedy: "check file permissions",
        },
        Refusal {
            code: "not_a_dsgrid_package",
            when: "the bytes are not a readable .dsgrid container",
            remedy: "a .dsgrid is a zip containing manifest.json; convert other formats first",
        },
        Refusal {
            code: "manifest_unreadable",
            when: "the package manifest does not match this build's schema",
            remedy: "rebuild the package with a matching ds-network release",
        },
        Refusal {
            code: "invalid_limit",
            when: "--limit is not a whole number in 1..5000",
            remedy: "pass a limit inside the range, or omit it for the default of 50",
        },
    ],
    reference: Some("docs/reference/dsgrid.md"),
    availability: available,
};

fn available() -> Availability {
    Availability::Available
}

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let raw_path = inputs.require("model")?;
    let limit = package::parse_limit(inputs.value("limit"))?;
    let bytes = package::read_bytes(raw_path)?;

    // The container answer. A decode failure is a *result*, not a refusal:
    // "this package is damaged" is exactly what the caller asked, and
    // answering it is this command working correctly.
    let package = match package::decode(raw_path, &bytes) {
        Ok(package) => package,
        Err(failure) => {
            return Ok(json!({
                "path": raw_path,
                "byte_len": bytes.len(),
                "container": {
                    "verified": false,
                    "detail": failure.detail_value(),
                },
                // Deliberately null rather than absent: the model was not
                // judged sound, and it was not judged unsound either.
                "model": Value::Null,
            }));
        }
    };

    let report = validate_snapshot(&package.snapshot);
    let issues: Vec<Value> = report
        .issues
        .iter()
        .map(|issue| {
            json!({
                "code": format!("{:?}", issue.code),
                "table": issue.table.map(package::table_token),
                "entity": issue.entity,
                "message": issue.message,
            })
        })
        .collect();

    let total = issues.len();
    let (shown, withheld) = package::take(issues, limit);

    let mut answer = json!({
        "path": raw_path,
        "byte_len": bytes.len(),
        "container": {
            "verified": true,
            "members": package.manifest.members.len(),
        },
        "model": {
            "id": package.manifest.model.model_id.as_str(),
            "revision": package.manifest.model.model_revision,
            "fingerprint": package.manifest.model.snapshot_fingerprint,
            "valid": total == 0,
            "issue_count": total,
            "issues": shown,
        },
    });

    if withheld > 0 {
        answer["more"] = json!({
            "truncated": [{ "field": "model.issues", "withheld": withheld, "limit": limit }],
        });
    }

    Ok(answer)
}

pub fn render(data: &Value) -> String {
    if !data["container"]["verified"].as_bool().unwrap_or(false) {
        return format!(
            "container  DAMAGED\n  {}\n\nThe model could not be read, so it was not validated.",
            data["container"]["detail"]["detail"].as_str().unwrap_or(""),
        );
    }

    let model = &data["model"];
    let mut out = format!(
        "container  verified ({} members)\nmodel      {}  rev {}\n",
        data["container"]["members"],
        model["id"].as_str().unwrap_or("?"),
        model["revision"],
    );

    if model["valid"].as_bool().unwrap_or(false) {
        out.push_str("           valid — no issues\n");
        return out;
    }

    out.push_str(&format!("           {} issue(s)\n\n", model["issue_count"]));
    for issue in model["issues"].as_array().into_iter().flatten() {
        out.push_str(&format!(
            "  {:<24} {}\n",
            issue["code"].as_str().unwrap_or(""),
            issue["message"].as_str().unwrap_or(""),
        ));
        if let Some(entity) = issue["entity"].as_str() {
            out.push_str(&format!("  {:<24}   {entity}\n", ""));
        }
    }
    if let Some(truncated) = data["more"]["truncated"]
        .as_array()
        .and_then(|list| list.first())
    {
        out.push_str(&format!(
            "\n  … {} more withheld by --limit\n",
            truncated["withheld"]
        ));
    }
    out
}
