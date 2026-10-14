//! `ds dsgrid model show` — one working copy, with the head revision the
//! engine reads off its package right now.
//!
//! `model list` prints what the catalogue row carries; this command opens
//! the package beside the row and reports the live authored head, so a
//! caller pinning `--revision` for a typed mutation reads it from here.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Availability, Chapter, Command, Effect, Example, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Value, json};

use crate::model::workspace;

const MODEL_ARG: Arg = Arg::value(
    "model",
    "<local-id>",
    "A working copy on this machine, by the id `ds dsgrid model list` reports.",
)
.required();
const ACCOUNT_ARG: Arg = Arg::value(
    "account",
    "<uid>",
    "The DS account whose catalogue holds the copy; omitted, the id is looked up across this machine's catalogues.",
);

const BASE_REFUSALS: &[Refusal] = &[
    Refusal {
        code: "local_model_not_found",
        when: "no working copy on this machine carries that id",
        remedy: "run `ds dsgrid model list` and use an id from it",
    },
    Refusal {
        code: "local_model_ambiguous",
        when: "the id was found in more than one account's catalogue on this machine",
        remedy: "pass --account <uid> to say whose working copy you mean",
    },
    Refusal {
        code: "local_model_store_unavailable",
        when: "the machine's catalogue or the package beside it cannot be read",
        remedy: "check the local data directory; DS_LAYER_HOME may name an absolute shared directory",
    },
    Refusal {
        code: "model_not_found",
        when: "the row exists but its package file is missing",
        remedy: "the copy is damaged; re-import it with `ds dsgrid model import-external`",
    },
    Refusal {
        code: "package_decode_failed",
        when: "the package is damaged or carries a table schema this build does not decode",
        remedy: "re-convert it from its PLS-CADD workspace with `ds dsgrid-exchange convert`, then `ds dsgrid model import-external`",
    },
    Refusal {
        code: "not_a_dsgrid_package",
        when: "the bytes beside the row are not a .dsgrid container",
        remedy: "the copy is damaged; re-import it",
    },
    Refusal {
        code: "model_unreadable",
        when: "the package exists but cannot be read",
        remedy: "check file permissions",
    },
    Refusal {
        code: "model_too_large",
        when: "the package is above the 512 MiB read bound",
        remedy: "confirm the file is a .dsgrid package",
    },
];

const REFUSALS: &[Refusal] = &crate::native::extend_refusals::<
    { BASE_REFUSALS.len() + crate::native::REFUSALS.len() },
>(BASE_REFUSALS);

pub static COMMAND: Command = Command {
    id: "dsgrid.model.show",
    path: &["dsgrid", "model", "show"],
    contract: 1,
    summary: "Show one working copy and the live head revision of its package.",
    purpose: "\
Reads one working copy's catalogue row and opens the package beside it with \
the engine, so the answer carries the live authored head (`rev:…`) every \
typed mutation pins against, the package revision, the content digest, the \
table counts an engineer expects (alignments, structures, structure types, \
sections), the copy's `pls_source` link (present and empty until contract 02 \
lands) and its project binding when it was taken from a project.",
    chapter: Chapter::GridModel,
    effect: Effect::ReadOnly,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[MODEL_ARG, workspace::LANE_ARG, ACCOUNT_ARG],
    output: "\
The catalogue row (`model`, `name`, `origin`, `crs`, `revision`, \
`content_digest`, `head_revision`, `revised_at`, `pls_source`, \
`project_binding`) plus `head` {authored_revision, package_revision, \
model_id, fingerprint, valid, issue_count} read live, `counts` per table, \
and `head_matches_row` — false when the row's recorded head is stale.",
    examples: &[Example {
        command: "ds dsgrid model show --model local-… --output json",
        note: "Read .data.head.authored_revision to pin a typed mutation.",
        runnable: false,
    }],
    refusals: REFUSALS,
    reference: Some("docs/reference/dsgrid.md"),
    search: &["working copy", "local model", "pin"],
    requires: Requires::Server,
    availability: || Availability::Available,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    use ds_network_contracts::grid_read::{
        NATIVE_GRID_READ_REQUEST_SCHEMA_V1, NativeGridReadRequestV1,
    };

    let located = workspace::locate(inputs, inputs.require("model")?)?;
    let path = located.path.display().to_string();
    let bytes = crate::package::read_bytes(&path)?;
    let request = NativeGridReadRequestV1 {
        schema: NATIVE_GRID_READ_REQUEST_SCHEMA_V1.into(),
        issue_limit: 0,
        advisory_limit: 0,
    };
    let scratch = crate::native::scratch()?;
    let result = crate::native::capture()?
        .grid_read(&request, &bytes, scratch.path(), &|| false)
        .map_err(crate::native::map_failure)?
        .result;
    if !result.container.verified {
        return Err(
            Failure::failed("package_decode_failed", format!("`{path}` did not decode"))
                .remedy("the canonical reader accepts current schema only; retain the original and use `ds dsgrid package migrate --path <source> --expected-source-sha256 <capture> --dry-run`, then open the saved current package")
                .detail(result.container.detail.unwrap_or(Value::Null)),
        );
    }
    let head = result.model.ok_or_else(|| {
        Failure::failed(
            "network_result_invalid",
            "Network omitted model facts for a verified package",
        )
    })?;
    let authored_revision = head.authored_revision.clone();
    let mut row = workspace::row(&located.row, None);
    row["active"] = Value::Null;
    Ok(json!({
        "lane": located.scope.lane,
        "account": located.scope.uid,
        "path": path,
        "row": row,
        "head": {
            "authored_revision": authored_revision,
            "package_revision": head.package_revision,
            "model_id": head.id,
            "fingerprint": head.fingerprint,
            "valid": head.valid,
            "issue_count": head.issue_count,
        },
        "head_matches_row": located.row.head_revision.as_deref() == Some(authored_revision.as_str()),
        "counts": head.counts,
    }))
}

pub fn render(data: &Value) -> String {
    let row = &data["row"];
    format!(
        "{} · {}\n  head       {}{}\n  package    rev {} · {}\n  structures {} ({} described) · alignments {} · sections {}\n  pls_source {}\n",
        row["model"].as_str().unwrap_or("?"),
        row["name"].as_str().unwrap_or(""),
        data["head"]["authored_revision"].as_str().unwrap_or("?"),
        if data["head_matches_row"].as_bool().unwrap_or(false) {
            ""
        } else {
            "  (row recorded another head)"
        },
        data["head"]["package_revision"],
        row["content_digest"]
            .as_str()
            .map(|d| format!("sha256:{}…", &d[..12.min(d.len())]))
            .unwrap_or_default(),
        data["counts"]["structures"],
        data["counts"]["described_structures"],
        data["counts"]["alignments"],
        data["counts"]["tension_sections"],
        match row["pls_source"].as_object() {
            None => "none — `ds dsgrid model link` to pin a PLS-CADD workspace".to_string(),
            Some(link) => {
                let versions = link
                    .get("member_versions")
                    .and_then(Value::as_object)
                    .map(|versions| {
                        versions
                            .iter()
                            .map(|(family, version)| {
                                format!("{family} {}", version.as_str().unwrap_or("?"))
                            })
                            .collect::<Vec<_>>()
                            .join(" · ")
                    })
                    .unwrap_or_default();
                format!(
                    "{}\n             PLS-CADD {} · {} members · {}\n             {}",
                    link.get("path").and_then(Value::as_str).unwrap_or("?"),
                    link.get("pls_version")
                        .and_then(Value::as_str)
                        .unwrap_or("?"),
                    link.get("member_count").cloned().unwrap_or(Value::Null),
                    versions,
                    link.get("digest").and_then(Value::as_str).unwrap_or("?"),
                )
            }
        },
    )
}
