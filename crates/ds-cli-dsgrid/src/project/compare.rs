//! `ds dsgrid project compare` — what changed between two model revisions.
//!
//! Each side is an exact project revision, downloaded and digest-verified by
//! the native owner, or a local `.dsgrid` package. Both are fully verified by
//! `ds-grid-exchange` before any table is read, and the difference is
//! `ds_grid_engine::diff_snapshots` — the same pure snapshot diff the
//! Desktop's version history compares with. Nothing is computed here; the
//! answer is bounded per table with its truncation stated.
use super::{LANE, SHARED, with_shared};
use ds_cli_contract::spec::{
    Arg, Authority, Chapter, Command, Effect, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Failure, Inputs};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

const SIDE_INVALID: Refusal = Refusal {
    code: "compare_side_invalid",
    when: "a side names both or neither of a revision and a path, or a revision without --project and --model",
    remedy: "name --from or --from-path, and --to or --to-path; revisions need --project and --model",
};
const LIMIT_INVALID: Refusal = Refusal {
    code: "grid_project_output_invalid",
    when: "--limit is outside 1..500",
    remedy: "pass --limit from 1 to 500 ids per table and change kind",
};
const DECODE: Refusal = Refusal {
    code: "package_decode_failed",
    when: "a side's package does not verify and decode",
    remedy: "inspect the package with ds dsgrid validate; re-download or re-convert it",
};
const OWN: usize = 3 + 4;
const REFUSALS: &[Refusal] = &{
    let mut r = with_shared([SIDE_INVALID; OWN + SHARED], OWN);
    r[1] = LIMIT_INVALID;
    r[2] = DECODE;
    let mut n = 0;
    while n < crate::package::SHARED_REFUSALS.len() {
        r[3 + n] = crate::package::SHARED_REFUSALS[n];
        n += 1;
    }
    r
};

pub static COMMAND: Command = Command {
    id: "dsgrid.project.compare",
    path: &["dsgrid", "project", "compare"],
    contract: 1,
    summary: "Compare two model revisions: entities added, changed, removed.",
    purpose: "Answer what a save or a submission changed. Each side is an exact project revision (downloaded and digest-verified) or a local .dsgrid, so a working copy can be compared with the head it came from. Returns per-table added, changed and removed entity ids from the engine's snapshot diff, plus both sides' model identity and fingerprint. Ids are bounded per table by --limit with counts always complete.",
    chapter: Chapter::GridModel,
    effect: Effect::LocalAuthState,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[
        Arg::value(
            "project",
            "<ds-project>",
            "Exact project; required when a side is a revision.",
        ),
        LANE,
        Arg::value(
            "model",
            "<id>",
            "Exact project model; required when a side is a revision.",
        ),
        Arg::value("from", "<revision-id>", "Earlier side: a project revision."),
        Arg::value(
            "from-path",
            "<file.dsgrid>",
            "Earlier side: a local package.",
        ),
        Arg::value("to", "<revision-id>", "Later side: a project revision."),
        Arg::value("to-path", "<file.dsgrid>", "Later side: a local package."),
        Arg::value(
            "limit",
            "<1..500>",
            "Ids listed per table and change kind; counts are always complete.",
        )
        .default("20"),
    ],
    output: "from/to {kind, revision_id|path, sha256, model_id, model_revision, snapshot_fingerprint}, identical, totals {added, changed, removed}, changed_tables, tables[] {table, added|changed|removed counts and first --limit ids, truncated}, changed_dependency_tokens (bounded) and their total.",
    examples: &[],
    refusals: REFUSALS,
    reference: Some("docs/reference/dsgrid.md"),
    search: &["diff revisions", "what changed", "compare versions"],
    requires: Requires::Server,
    availability: || ds_cli_contract::spec::Availability::Available,
};

fn side(i: &Inputs, revision: &str, path: &str) -> Result<(Vec<u8>, Value), Failure> {
    match (i.value(revision), i.value(path)) {
        (Some(revision), None) => {
            let (Some(project), Some(model)) = (i.value("project"), i.value("model")) else {
                return Err(side_invalid("a revision side needs --project and --model"));
            };
            let mut downloaded = ds_cli_auth::grid_models_for_project(
                i.require("lane")?,
                project,
                &ds_cli_auth::GridModelsCommand::Download {
                    model: model.into(),
                    revision: revision.into(),
                },
            )?;
            let bytes = downloaded
                .bytes
                .take()
                .ok_or_else(|| side_invalid("verified owner returned no package"))?;
            Ok((
                bytes,
                json!({"kind":"revision","project":project,"model":model,"revision_id":revision,
                    "sha256":downloaded.data["sha256"],"bytes":downloaded.data["bytes"]}),
            ))
        }
        (None, Some(path)) => {
            let bytes = crate::package::read_bytes(path)?;
            let sha256 = format!("{:x}", Sha256::digest(&bytes));
            let length = bytes.len();
            Ok((
                bytes,
                json!({"kind":"path","path":path,"sha256":sha256,"bytes":length}),
            ))
        }
        _ => Err(side_invalid(&format!(
            "name exactly one of --{revision} and --{path}"
        ))),
    }
}

fn side_invalid(message: &str) -> Failure {
    Failure::invalid("compare_side_invalid", message.to_owned()).remedy(SIDE_INVALID.remedy)
}

pub fn run(i: &Inputs, _: &Context) -> Result<Value, Failure> {
    let limit = i
        .require("limit")?
        .parse::<usize>()
        .ok()
        .filter(|n| (1..=500).contains(n))
        .ok_or_else(|| {
            Failure::invalid("grid_project_output_invalid", "--limit must be 1..500")
                .remedy(LIMIT_INVALID.remedy)
        })?;
    // Both sides are named before either is read, so a malformed request
    // never costs a download.
    for (revision, path) in [("from", "from-path"), ("to", "to-path")] {
        if i.value(revision).is_some() == i.value(path).is_some() {
            return Err(side_invalid(&format!(
                "name exactly one of --{revision} and --{path}"
            )));
        }
    }
    let (from_bytes, mut from) = side(i, "from", "from-path")?;
    let (to_bytes, mut to) = side(i, "to", "to-path")?;
    let label = |side: &Value| {
        side["revision_id"]
            .as_str()
            .or(side["path"].as_str())
            .unwrap_or("?")
            .to_owned()
    };
    let a = crate::package::decode(&label(&from), &from_bytes)?;
    let b = crate::package::decode(&label(&to), &to_bytes)?;
    for (side, package) in [(&mut from, &a), (&mut to, &b)] {
        side["model_id"] = json!(package.manifest.model.model_id);
        side["model_revision"] = json!(package.manifest.model.model_revision);
        side["snapshot_fingerprint"] = json!(package.snapshot.snapshot_fingerprint());
    }
    let diff = ds_grid_engine::diff_snapshots(&a.snapshot, &b.snapshot);
    let tables: Vec<Value> = diff
        .table_diffs
        .iter()
        .map(|table| {
            let bounded = |ids: &Vec<String>| ids.iter().take(limit).cloned().collect::<Vec<_>>();
            let truncated = [&table.added, &table.changed, &table.removed]
                .iter()
                .any(|ids| ids.len() > limit);
            json!({
                "table": crate::package::table_token(table.table),
                "added_count": table.added.len(),
                "changed_count": table.changed.len(),
                "removed_count": table.removed.len(),
                "added": bounded(&table.added),
                "changed": bounded(&table.changed),
                "removed": bounded(&table.removed),
                "truncated": truncated,
            })
        })
        .collect();
    let tokens = &diff.changed_dependency_tokens;
    let changed_tables: Vec<String> = diff
        .changed_tables
        .iter()
        .map(|t| crate::package::table_token(*t))
        .collect();
    let bounded_tokens: Vec<_> = tokens.iter().take(limit).collect();
    Ok(json!({
        "from": from,
        "to": to,
        "identical": diff.is_empty(),
        "totals": {"added": diff.added_total, "changed": diff.changed_total, "removed": diff.removed_total},
        "changed_tables": changed_tables,
        "tables": tables,
        "changed_dependency_tokens": bounded_tokens,
        "changed_dependency_tokens_total": tokens.len(),
        "limit": limit,
    }))
}

pub fn render(v: &Value) -> String {
    let mut out = format!(
        "{} → {}: {}\n",
        v["from"]["revision_id"]
            .as_str()
            .or(v["from"]["path"].as_str())
            .unwrap_or("?"),
        v["to"]["revision_id"]
            .as_str()
            .or(v["to"]["path"].as_str())
            .unwrap_or("?"),
        if v["identical"] == true {
            "identical".to_owned()
        } else {
            format!(
                "+{} ~{} -{}",
                v["totals"]["added"], v["totals"]["changed"], v["totals"]["removed"]
            )
        }
    );
    for table in v["tables"].as_array().into_iter().flatten() {
        out.push_str(&format!(
            "  {:<28} +{} ~{} -{}{}\n",
            table["table"].as_str().unwrap_or("?"),
            table["added_count"],
            table["changed_count"],
            table["removed_count"],
            if table["truncated"] == true {
                "  (ids truncated)"
            } else {
                ""
            }
        ));
    }
    out
}
