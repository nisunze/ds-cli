<<<<<<< HEAD
//! `ds dsgrid model show` — one working copy, with the head revision the
//! engine reads off its package right now.
//!
//! `model list` prints what the catalogue row carries; this command opens
//! the package beside the row and reports the live authored head, so a
//! caller pinning `--revision` for a typed mutation reads it from here.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Availability, Chapter, Command, Effect, Example, Execution, Refusal, Requires,
=======
//! `ds dsgrid model show` — one working copy in full: its catalogue row,
//! the identity its package declares, and the PLS-CADD workspace it is
//! linked to.
//!
//! `list` is the index; this is the record. It opens the package through the
//! engine so what it prints about revision and coordinate system is what the
//! bytes say now, beside what the row recorded when the copy was acquired —
//! a copy edited in place shows both.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, ArgKind, Authority, Availability, Chapter, Command, Effect, Example, Execution, Refusal,
    Requires,
>>>>>>> origin/program/02-d6
};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Value, json};

<<<<<<< HEAD
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

const REFUSALS: &[Refusal] = &[
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
=======
use crate::model::{pls_source, workspace};

const MODEL_ARG: Arg = Arg {
    name: "model",
    kind: ArgKind::Value,
    value: "<model-id>",
    required: true,
    default: None,
    choices: &[],
    summary: "The working copy, by the id `ds dsgrid model list` reports.",
};

const SHOW_OWN: [Refusal; 1] = [Refusal {
    code: "not_a_dsgrid_package",
    when: "the store holds bytes this build's engine cannot open as a package",
    remedy: "re-import the package; a package written by a newer schema is refused, not guessed",
}];
const REFUSALS: &[Refusal; SHOW_OWN.len() + workspace::REFUSALS.len()] = &refusals();
const fn refusals() -> [Refusal; SHOW_OWN.len() + workspace::REFUSALS.len()] {
    let mut all = [SHOW_OWN[0]; SHOW_OWN.len() + workspace::REFUSALS.len()];
    let mut shared = 0;
    while shared < workspace::REFUSALS.len() {
        all[SHOW_OWN.len() + shared] = workspace::REFUSALS[shared];
        shared += 1;
    }
    all
}
>>>>>>> origin/program/02-d6

pub static COMMAND: Command = Command {
    id: "dsgrid.model.show",
    path: &["dsgrid", "model", "show"],
    contract: 1,
<<<<<<< HEAD
    summary: "Show one working copy and the live head revision of its package.",
    purpose: "\
Reads one working copy's catalogue row and opens the package beside it with \
the engine, so the answer carries the live authored head (`rev:…`) every \
typed mutation pins against, the package revision, the content digest, the \
table counts an engineer expects (alignments, structures, structure types, \
sections), the copy's `pls_source` link (present and empty until contract 02 \
lands) and its project binding when it was taken from a project.",
=======
    summary: "Show one working copy: row, package identity, PLS-CADD link.",
    purpose: "\
Prints one of this machine's working copies in full: the catalogue row \
(name, origin, revision, digest, project pin, head revision), what its \
package declares now (model id, package revision, coordinate system, table \
counts), and the PLS-CADD workspace it is linked to with the member versions \
the link pinned. Reads the package through the engine; writes nothing.",
>>>>>>> origin/program/02-d6
    chapter: Chapter::GridModel,
    effect: Effect::ReadOnly,
    authority: Authority::None,
    execution: Execution::Sync,
<<<<<<< HEAD
    args: &[MODEL_ARG, workspace::LANE_ARG, ACCOUNT_ARG],
    output: "\
The catalogue row (`model`, `name`, `origin`, `crs`, `revision`, \
`content_digest`, `head_revision`, `revised_at`, `pls_source`, \
`project_binding`) plus `head` {authored_revision, package_revision, \
model_id, fingerprint, valid, issue_count, prior_schema_members} read live, `counts` per table, \
and `head_matches_row` — false when the row's recorded head is stale.",
    examples: &[Example {
        command: "ds dsgrid model show --model local-… --output json",
        note: "Read .data.head.authored_revision to pin a typed mutation.",
=======
    args: &[MODEL_ARG, workspace::LANE_ARG, workspace::ACCOUNT_ARG],
    output: "\
The `model` row with `pls_source`, and `package` {model_id, package_revision, \
crs, structures, structure_types, tension_sections, terrain_points, bytes, \
sha256, path}.",
    examples: &[Example {
        command: "ds dsgrid model show --model local-5ff16cd0a3d6416b --account <uid> --output json",
        note: "Read .data.model.pls_source to see where a sync would write.",
>>>>>>> origin/program/02-d6
        runnable: false,
    }],
    refusals: REFUSALS,
    reference: Some("docs/reference/dsgrid.md"),
<<<<<<< HEAD
    search: &["working copy", "local model", "pin"],
=======
    search: &["working copy", "linked workspace", "details"],
>>>>>>> origin/program/02-d6
    requires: Requires::Server,
    availability: || Availability::Available,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
<<<<<<< HEAD
    let located = workspace::locate(inputs, inputs.require("model")?)?;
    let path = located.path.display().to_string();
    let bytes = crate::package::read_bytes(&path)?;
    let package = crate::package::decode(&path, &bytes)?;
    let prior_schema_members = crate::package::prior_schema_members(&package.manifest);
    let session = ds_grid_engine::GridSession::open(package.snapshot);
    let head = session.current_revision().revision_id.as_str().to_string();
    let snapshot = session.snapshot();
    let mut row = workspace::row(&located.row, None);
    row["active"] = Value::Null;
    Ok(json!({
        "lane": located.scope.lane,
        "account": located.scope.uid,
        "path": path,
        "row": row,
        "head": {
            "authored_revision": head,
            "package_revision": package.manifest.model.model_revision,
            "model_id": package.manifest.model.model_id.as_str(),
            "fingerprint": package.manifest.model.snapshot_fingerprint,
            "valid": session.is_valid(),
            "issue_count": session.validation().issues.len(),
            "prior_schema_members": prior_schema_members,
        },
        "head_matches_row": located.row.head_revision.as_deref() == Some(head.as_str()),
        "counts": {
            "alignments": snapshot.alignments.len(),
            "route_nodes": snapshot.route_nodes.len(),
            "structures": snapshot.structures.len(),
            "structure_types": snapshot.structure_types.len(),
            "tension_sections": snapshot.tension_sections.len(),
            "described_structures": snapshot.structures.iter().filter(|s| s.description.is_some()).count(),
=======
    let id = inputs.require("model")?.trim().to_owned();
    let opened = pls_source::open(inputs, &id)?;
    let snapshot = &opened.package.snapshot;
    Ok(json!({
        "model": workspace::row(&opened.row, opened.active.as_deref()),
        "package": {
            "model_id": opened.package.manifest.model.model_id.as_str(),
            "package_revision": opened.package.manifest.model.model_revision,
            "crs": opened.package.manifest.model.coordinate_system.to_string(),
            "structures": snapshot.structures.len(),
            "structure_types": snapshot.structure_types.len(),
            "tension_sections": snapshot.tension_sections.len(),
            "terrain_points": snapshot.terrain_points.len(),
            "bytes": opened.package_bytes.len(),
            "sha256": ds_io::pls_cadd_native::sha256_hex_digest(&opened.package_bytes),
            "path": opened.package_path.to_string_lossy(),
>>>>>>> origin/program/02-d6
        },
    }))
}

pub fn render(data: &Value) -> String {
<<<<<<< HEAD
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
        if row["pls_source"].is_null() {
            "none (not linked)"
        } else {
            "linked"
        },
    )
=======
    let model = &data["model"];
    let package = &data["package"];
    let mut out = format!(
        "{}{} · {}\n  origin     {}\n  revision   {}   head {}\n  crs        {}\n  package    rev {} · {} bytes · {}\n  content    {} structures · {} types · {} sections · {} terrain points\n",
        if model["active"].as_bool().unwrap_or(false) {
            "* "
        } else {
            ""
        },
        model["model"].as_str().unwrap_or("?"),
        model["name"].as_str().unwrap_or(""),
        model["origin"].as_str().unwrap_or("?"),
        model["revision"],
        model["head_revision"].as_str().unwrap_or("—"),
        package["crs"].as_str().unwrap_or("?"),
        package["package_revision"],
        package["bytes"],
        package["path"].as_str().unwrap_or("?"),
        package["structures"],
        package["structure_types"],
        package["tension_sections"],
        package["terrain_points"],
    );
    match model["pls_source"].as_object() {
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
            out.push_str(&format!(
                "  linked     {}\n             PLS-CADD {} · {} members · {}\n             {}\n",
                link.get("path").and_then(Value::as_str).unwrap_or("?"),
                link.get("pls_version")
                    .and_then(Value::as_str)
                    .unwrap_or("?"),
                link.get("member_count").cloned().unwrap_or(Value::Null),
                versions,
                link.get("digest").and_then(Value::as_str).unwrap_or("?"),
            ));
        }
        None => {
            out.push_str("  linked     none — `ds dsgrid model link` to pin a PLS-CADD workspace\n")
        }
    }
    out
>>>>>>> origin/program/02-d6
}
