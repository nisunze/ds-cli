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
    Arg, Authority, Availability, Chapter, Command, Effect, Example, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Value, json};

use crate::package;

const OWN_REFUSALS: &[Refusal] = &[
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
        code: "package_decode_failed",
        when: "the container is empty, truncated or damaged and cannot be opened or verified",
        remedy: "restore the package from its source, or re-export it as a current .dsgrid",
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
];
const REFUSALS: &[Refusal] = &crate::native::extend_refusals::<
    { OWN_REFUSALS.len() + crate::native::REFUSALS.len() },
>(OWN_REFUSALS);

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
are reported apart. Nonblocking owner advisories remain visible on valid models \
so inconsistent cable mechanics can be reviewed and repaired.",
    chapter: Chapter::GridModel,
    effect: Effect::Discovery,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[
        Arg::value("model", "<path>", "The .dsgrid package to verify.").required(),
        Arg::value("limit", "<n>", "Cap each issue and advisory list.")
            .default(package::DEFAULT_LIMIT),
    ],
    output: "\
`container` and `model`, each with its own verdict; a container that cannot be \
opened or verified is refused as package_decode_failed, never reported as a \
result. Model issues carry a stable \
code, the table and entity concerned, and a message. `more.truncated` reports \
any findings withheld by --limit. `model.advisories` carries nonblocking owner \
diagnostics with stable codes; `advisory_count` counts the complete list. \
Advisories do not change `model.valid`.",
    examples: &[Example {
        command: "ds dsgrid validate --model ./model.dsgrid --output json",
        note: "Exit 0 whether or not issues were found; read .data.model.valid.",
        runnable: false,
    }],
    refusals: REFUSALS,
    reference: Some("docs/reference/dsgrid.md"),
    search: &[],
    requires: Requires::Server,
    availability: available,
};

fn available() -> Availability {
    Availability::Available
}

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    use ds_network_contracts::grid_read::{
        NATIVE_GRID_READ_REQUEST_SCHEMA_V1, NativeGridReadRequestV1,
    };

    let raw_path = inputs.require("model")?;
    let limit = package::parse_limit(inputs.value("limit"))?;
    let bytes = package::read_bytes(raw_path)?;
    let request = NativeGridReadRequestV1 {
        schema: NATIVE_GRID_READ_REQUEST_SCHEMA_V1.into(),
        issue_limit: limit,
        advisory_limit: limit,
    };
    let scratch = crate::native::scratch()?;
    let result = crate::native::capture()?
        .grid_read(&request, &bytes, scratch.path(), &|| false)
        .map_err(crate::native::map_failure)?
        .result;

    if !result.container.verified {
        // A container that cannot be opened or verified is the caller's file:
        // the same refusal every other door gives it, never a clean answer.
        return Err(Failure::invalid(
            "package_decode_failed",
            format!("`{raw_path}` did not verify as a .dsgrid package"),
        )
        .remedy("the package is empty, truncated or damaged; restore it from its source, or re-export it as a current .dsgrid (`ds dsgrid package migrate` for an older one)")
        .detail(json!({
            "byte_len": bytes.len(),
            "container": result.container.detail.unwrap_or(Value::Null),
        })));
    }

    let model = result.model.ok_or_else(|| {
        Failure::failed(
            "network_result_invalid",
            "Network omitted model facts for a verified package",
        )
    })?;
    let issues = model
        .issues
        .into_iter()
        .map(|issue| {
            json!({
                "code": issue.code,
                "table": issue.table,
                "entity": issue.entity,
                "message": issue.message,
            })
        })
        .collect::<Vec<_>>();
    let advisories = model
        .advisories
        .into_iter()
        .map(|issue| {
            json!({
                "code": issue.code,
                "table": issue.table,
                "entity": issue.entity,
                "message": issue.message,
            })
        })
        .collect::<Vec<_>>();

    let mut answer = json!({
        "path": raw_path,
        "byte_len": bytes.len(),
        "container": {
            "verified": true,
            "members": result.container.members,
        },
        "model": {
            "id": model.id,
            "revision": model.package_revision,
            "authored_revision": model.authored_revision,
            "fingerprint": model.fingerprint,
            "valid": model.valid,
            "issue_count": model.issue_count,
            "issues": issues,
            "advisory_count": model.advisory_count,
            "advisories": advisories,
        },
    });

    let truncated: Vec<Value> = [
        ("model.issues", model.issues_withheld),
        ("model.advisories", model.advisories_withheld),
    ]
    .into_iter()
    .filter(|(_, count)| *count > 0)
    .map(|(field, withheld)| json!({"field": field, "withheld": withheld, "limit": limit}))
    .collect();
    if !truncated.is_empty() {
        answer["more"] = json!({ "truncated": truncated });
    }

    Ok(answer)
}

pub fn render(data: &Value) -> String {
    let model = &data["model"];
    let mut out = format!(
        "container  verified ({} members)\nmodel      {}  rev {}\n",
        data["container"]["members"],
        model["id"].as_str().unwrap_or("?"),
        model["revision"],
    );

    if model["valid"].as_bool().unwrap_or(false) {
        out.push_str("           valid — no blocking issues\n");
    } else {
        out.push_str(&format!("           {} issue(s)\n", model["issue_count"]));
    }
    if model["advisory_count"]
        .as_u64()
        .is_some_and(|count| count > 0)
    {
        out.push_str(&format!(
            "           {} advisory finding(s)\n",
            model["advisory_count"]
        ));
    }
    for issue in ["issues", "advisories"]
        .into_iter()
        .flat_map(|field| model[field].as_array().into_iter().flatten())
    {
        out.push_str(&format!(
            "  {:<24} {}\n",
            issue["code"].as_str().unwrap_or(""),
            issue["message"].as_str().unwrap_or(""),
        ));
        if let Some(entity) = issue["entity"].as_str() {
            out.push_str(&format!("  {:<24}   {entity}\n", ""));
        }
    }
    for truncated in data["more"]["truncated"].as_array().into_iter().flatten() {
        out.push_str(&format!(
            "\n  … {} more in {} withheld by --limit\n",
            truncated["withheld"],
            truncated["field"].as_str().unwrap_or("")
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_valid_model_still_renders_its_modulus_advisory_and_truncation() {
        let data = json!({
            "container": {"verified": true, "members": 64},
            "model": {
                "id": "model", "revision": 0, "valid": true, "issue_count": 0,
                "issues": [], "advisory_count": 2,
                "advisories": [{
                    "code": "CABLE_MODULUS_DISAGREEMENT", "entity": "cb-opgw",
                    "message": "nominal 68.9474 GPa differs from strand final 97.3 GPa"
                }]
            },
            "more": {"truncated": [{"field": "model.advisories", "withheld": 1, "limit": 1}]}
        });
        let text = render(&data);
        assert!(text.contains("valid"));
        assert!(text.contains("CABLE_MODULUS_DISAGREEMENT"));
        assert!(text.contains("cb-opgw"));
        assert!(text.contains("97.3 GPa"));
        assert!(text.contains("model.advisories"));
        assert!(text.contains("1 more"));
    }

    #[test]
    fn package_validation_exposes_bounded_owner_advisories_without_invalidating_it() {
        use ds_grid_exchange::package::{PackOptions, unpack};
        let mut package = unpack(include_bytes!(
            "../../../../ds-network/fixtures/pls-public/humble-pole/humble-pole.dsgrid"
        ))
        .unwrap();
        for cable in &mut package.snapshot.cables {
            cable.nominal_elastic_modulus_pa = Some(1.0);
        }
        ds_grid_engine::recompute_stored_geometry(&mut package.snapshot);
        let expected = ds_grid_model::validate_snapshot(&package.snapshot);
        assert!(expected.is_valid(), "{:?}", expected.issues);
        assert!(
            expected
                .advisories
                .iter()
                .filter(|a| a.code.as_str() == "CABLE_MODULUS_DISAGREEMENT")
                .count()
                >= 2
        );
        let (artifacts, _) = ds_grid_exchange::dsgrid::emit(
            &package.snapshot,
            &PackOptions {
                presentation: package.manifest.model.presentation,
                model_id: package.manifest.model.model_id,
                model_revision: package.manifest.model.model_revision,
                coordinate_system: package.manifest.model.coordinate_system,
                library_pins: package.manifest.model.library_pins,
                library_needs: package.manifest.model.library_needs,
                assets: package.assets,
                exchange_bindings: package.exchange_bindings,
            },
        )
        .unwrap();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("modulus.dsgrid");
        std::fs::write(&path, &artifacts.artifacts[0].bytes).unwrap();
        let inputs = ds_cli_contract::args::parse(
            &COMMAND,
            &[
                "--model".into(),
                path.to_str().unwrap().into(),
                "--limit".into(),
                "1".into(),
            ],
        )
        .unwrap();
        let data = run(
            &inputs,
            &Context {
                confirmed: false,
                output: ds_cli_contract::Output::resolve(
                    ds_cli_contract::Format::Json,
                    false,
                    true,
                ),
            },
        )
        .unwrap();
        assert_eq!(data["model"]["valid"], true);
        assert_eq!(data["model"]["issue_count"], 0);
        assert_eq!(data["model"]["advisory_count"], expected.advisories.len());
        assert_eq!(data["model"]["advisories"].as_array().unwrap().len(), 1);
        assert_eq!(
            data["model"]["advisories"][0]["code"],
            "CABLE_MODULUS_DISAGREEMENT"
        );
        assert_eq!(data["more"]["truncated"][0]["field"], "model.advisories");
        assert_eq!(
            data["more"]["truncated"][0]["withheld"],
            expected.advisories.len() - 1
        );
    }
}
